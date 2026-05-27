use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use bytes::{Buf, BufMut};
use http::uri::PathAndQuery;
use prost::Message as ProstMessage;
use prost_reflect::{
    DescriptorPool, DynamicMessage, FieldDescriptor, Kind, MessageDescriptor, MethodDescriptor,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tonic::codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};
use tonic::metadata::{AsciiMetadataKey, AsciiMetadataValue, MetadataMap, MetadataValue};
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};
use tonic::{Request, Status};

use crate::HttpHeader;

#[derive(Debug, Deserialize)]
pub struct GrpcRequestPayload {
    pub target: String,
    pub proto_path: String,
    #[serde(default)]
    pub import_paths: Vec<String>,
    pub service: String,
    pub method: String,
    pub message: String,
    #[serde(default)]
    pub metadata: Vec<GrpcMetadataPair>,
    #[serde(default)]
    pub use_tls: bool,
    #[serde(default)]
    pub authority: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GrpcMetadataPair {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Serialize)]
pub struct GrpcResponsePayload {
    pub status: i32,
    pub status_text: String,
    pub body: String,
    pub headers: Vec<HttpHeader>,
    pub elapsed_ms: u128,
    pub size_bytes: usize,
}

#[derive(Debug, Deserialize)]
pub struct ParseProtoPayload {
    pub proto_path: String,
    #[serde(default)]
    pub import_paths: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ProtoServiceDescriptor {
    pub name: String,
    pub methods: Vec<ProtoMethodDescriptor>,
}

#[derive(Debug, Serialize)]
pub struct ProtoMethodDescriptor {
    pub name: String,
    pub input_type: String,
    pub output_type: String,
    pub client_streaming: bool,
    pub server_streaming: bool,
    pub request_template: String,
}

pub async fn send_grpc_request(payload: GrpcRequestPayload) -> Result<GrpcResponsePayload, String> {
    send_grpc_request_inner(payload)
        .await
        .map_err(|e| format!("{e:#}"))
}

pub async fn parse_proto_services(
    payload: ParseProtoPayload,
) -> Result<Vec<ProtoServiceDescriptor>, String> {
    parse_proto_services_inner(payload)
        .await
        .map_err(|e| format!("{e:#}"))
}

async fn parse_proto_services_inner(
    payload: ParseProtoPayload,
) -> Result<Vec<ProtoServiceDescriptor>> {
    let proto_path = PathBuf::from(payload.proto_path.trim());
    if !proto_path.is_file() {
        return Err(anyhow!(
            ".proto 文件不存在或不可读：{}",
            proto_path.display()
        ));
    }

    let mut include_dirs: Vec<PathBuf> = payload
        .import_paths
        .iter()
        .map(|p| PathBuf::from(p.trim()))
        .filter(|p| !p.as_os_str().is_empty())
        .collect();
    if let Some(parent) = proto_path.parent() {
        if !include_dirs.iter().any(|p| p == parent) {
            include_dirs.push(parent.to_path_buf());
        }
    }

    // Heavy work runs on a blocking thread so we don't stall the async runtime.
    let pool = tokio::task::spawn_blocking(move || compile_proto(&proto_path, &include_dirs))
        .await
        .map_err(|e| anyhow!("解析任务失败：{e}"))?
        .context("编译 .proto 失败")?;

    let services = pool
        .services()
        .map(|svc| ProtoServiceDescriptor {
            name: svc.full_name().to_string(),
            methods: svc
                .methods()
                .map(|m| {
                    let input = m.input();
                    let template = build_request_template(&input);
                    ProtoMethodDescriptor {
                        name: m.name().to_string(),
                        input_type: input.full_name().to_string(),
                        output_type: m.output().full_name().to_string(),
                        client_streaming: m.is_client_streaming(),
                        server_streaming: m.is_server_streaming(),
                        request_template: template,
                    }
                })
                .collect(),
        })
        .collect();

    Ok(services)
}

/// Build a pretty-printed JSON skeleton for the given message type, ready to
/// be edited by the user. Recursive types are guarded against infinite loops.
fn build_request_template(desc: &MessageDescriptor) -> String {
    let mut visited = HashSet::new();
    let value = template_for_message(desc, &mut visited);
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
}

fn template_for_message(desc: &MessageDescriptor, visited: &mut HashSet<String>) -> Value {
    // Special-case common well-known types where the canonical JSON shape
    // differs from a naive field-by-field expansion.
    match desc.full_name() {
        "google.protobuf.Timestamp" => return Value::String("1970-01-01T00:00:00Z".into()),
        "google.protobuf.Duration" => return Value::String("0s".into()),
        "google.protobuf.Empty" => return Value::Object(Map::new()),
        "google.protobuf.Any" => {
            let mut obj = Map::new();
            obj.insert("@type".into(), Value::String("".into()));
            return Value::Object(obj);
        }
        "google.protobuf.FieldMask" => return Value::String("".into()),
        "google.protobuf.Struct" => return Value::Object(Map::new()),
        "google.protobuf.Value" => return Value::Null,
        "google.protobuf.ListValue" => return Value::Array(Vec::new()),
        "google.protobuf.StringValue" => return Value::String("".into()),
        "google.protobuf.BytesValue" => return Value::String("".into()),
        "google.protobuf.BoolValue" => return Value::Bool(false),
        "google.protobuf.DoubleValue"
        | "google.protobuf.FloatValue"
        | "google.protobuf.Int32Value"
        | "google.protobuf.Int64Value"
        | "google.protobuf.UInt32Value"
        | "google.protobuf.UInt64Value" => return Value::Number(0u32.into()),
        _ => {}
    }

    let full = desc.full_name().to_string();
    if !visited.insert(full.clone()) {
        return Value::Null;
    }
    let mut obj = Map::new();
    let mut oneof_seen: HashSet<String> = HashSet::new();
    for field in desc.fields() {
        if let Some(oneof) = field.containing_oneof() {
            let oneof_name = oneof.name().to_string();
            // Only emit the first variant of each oneof to keep the JSON valid.
            if !oneof_seen.insert(oneof_name) {
                continue;
            }
        }
        obj.insert(field.json_name().to_string(), template_for_field(&field, visited));
    }
    visited.remove(&full);
    Value::Object(obj)
}

fn template_for_field(field: &FieldDescriptor, visited: &mut HashSet<String>) -> Value {
    if field.is_map() {
        return Value::Object(Map::new());
    }
    if field.is_list() {
        return Value::Array(Vec::new());
    }
    match field.kind() {
        Kind::Double | Kind::Float => Value::Number(serde_json::Number::from_f64(0.0).unwrap()),
        Kind::Int32
        | Kind::Int64
        | Kind::Uint32
        | Kind::Uint64
        | Kind::Sint32
        | Kind::Sint64
        | Kind::Fixed32
        | Kind::Fixed64
        | Kind::Sfixed32
        | Kind::Sfixed64 => Value::Number(0u32.into()),
        Kind::Bool => Value::Bool(false),
        Kind::String => Value::String(String::new()),
        Kind::Bytes => Value::String(String::new()),
        Kind::Enum(enum_desc) => {
            if let Some(first) = enum_desc.values().next() {
                Value::String(first.name().to_string())
            } else {
                Value::Number(0u32.into())
            }
        }
        Kind::Message(msg) => template_for_message(&msg, visited),
    }
}

async fn send_grpc_request_inner(payload: GrpcRequestPayload) -> Result<GrpcResponsePayload> {
    let proto_path = PathBuf::from(payload.proto_path.trim());
    if !proto_path.is_file() {
        return Err(anyhow!(
            ".proto 文件不存在或不可读：{}",
            proto_path.display()
        ));
    }

    let mut include_dirs: Vec<PathBuf> = payload
        .import_paths
        .iter()
        .map(|p| PathBuf::from(p.trim()))
        .filter(|p| !p.as_os_str().is_empty())
        .collect();

    if let Some(parent) = proto_path.parent() {
        if !include_dirs.iter().any(|p| p == parent) {
            include_dirs.push(parent.to_path_buf());
        }
    }

    let pool = compile_proto(&proto_path, &include_dirs)
        .context("编译 .proto 失败")?;

    let service_name = payload.service.trim();
    let method_name = payload.method.trim();

    let (method_desc, input_desc, output_desc, full_path) =
        resolve_method(&pool, service_name, method_name)?;

    let json_text = payload.message.trim();
    let json_text = if json_text.is_empty() { "{}" } else { json_text };
    let mut de = serde_json::Deserializer::from_str(json_text);
    let input_msg = DynamicMessage::deserialize(input_desc.clone(), &mut de)
        .context("JSON 无法转换为目标 message 类型")?;
    de.end().context("请求消息不是合法 JSON")?;

    let _ = method_desc;

    let endpoint = build_endpoint(&payload.target, payload.use_tls, payload.authority.as_deref())?;
    let channel = endpoint
        .connect()
        .await
        .with_context(|| format!("连接 {} 失败", payload.target))?;

    let mut request = Request::new(input_msg);
    apply_metadata(request.metadata_mut(), &payload.metadata)?;

    let path = PathAndQuery::from_maybe_shared(full_path).context("构造请求路径失败")?;
    let codec = DynCodec {
        output: output_desc.clone(),
    };

    let started = Instant::now();
    let mut client = tonic::client::Grpc::new(channel);
    client
        .ready()
        .await
        .map_err(|e| anyhow!("client not ready: {e}"))?;

    let result = client.unary(request, path, codec).await;
    let elapsed_ms = started.elapsed().as_millis();

    match result {
        Ok(response) => {
            let headers = metadata_to_headers(response.metadata());
            let message = response.into_inner();
            let json = dynamic_message_to_json(&message)?;
            let size = json.as_bytes().len();
            Ok(GrpcResponsePayload {
                status: 0,
                status_text: "OK".into(),
                body: json,
                headers,
                elapsed_ms,
                size_bytes: size,
            })
        }
        Err(status) => {
            let mut headers = metadata_to_headers(status.metadata());
            headers.push(HttpHeader {
                key: "grpc-message".into(),
                value: status.message().to_string(),
            });
            let body = serde_json::json!({
                "code": status.code() as i32,
                "code_name": format!("{:?}", status.code()),
                "message": status.message(),
            })
            .to_string();
            let size = body.as_bytes().len();
            Ok(GrpcResponsePayload {
                status: status.code() as i32,
                status_text: format!("{:?}", status.code()),
                body,
                headers,
                elapsed_ms,
                size_bytes: size,
            })
        }
    }
}

fn compile_proto(proto_path: &Path, include_dirs: &[PathBuf]) -> Result<DescriptorPool> {
    let mut compiler = protox::Compiler::new(include_dirs.iter().map(|p| p.as_path()))?;
    compiler.include_imports(true);
    compiler.include_source_info(true);
    compiler.open_file(proto_path)?;
    let fd_set = compiler.file_descriptor_set();
    let pool = DescriptorPool::from_file_descriptor_set(fd_set)
        .context("基于 .proto 描述符构建 DescriptorPool 失败")?;
    Ok(pool)
}

fn resolve_method(
    pool: &DescriptorPool,
    service: &str,
    method: &str,
) -> Result<(MethodDescriptor, MessageDescriptor, MessageDescriptor, String)> {
    let svc = pool
        .get_service_by_name(service)
        .ok_or_else(|| anyhow!("在 .proto 中找不到 service：{}", service))?;
    let mtd = svc
        .methods()
        .find(|m| m.name() == method)
        .ok_or_else(|| anyhow!("在 service {} 中找不到 method：{}", service, method))?;
    if mtd.is_client_streaming() || mtd.is_server_streaming() {
        return Err(anyhow!(
            "当前仅支持一元（unary）调用，{} 是流式 method",
            method
        ));
    }
    let input = mtd.input();
    let output = mtd.output();
    let full = format!("/{}/{}", svc.full_name(), mtd.name());
    Ok((mtd, input, output, full))
}

fn build_endpoint(target: &str, use_tls: bool, authority: Option<&str>) -> Result<Endpoint> {
    let scheme = if use_tls { "https" } else { "http" };
    let uri = format!("{scheme}://{target}");
    let mut endpoint = Channel::from_shared(uri)
        .map_err(|e| anyhow!("非法 target「{target}」：{e}"))?;
    if use_tls {
        let mut tls = ClientTlsConfig::new().with_native_roots();
        if let Some(auth) = authority {
            if !auth.is_empty() {
                tls = tls.domain_name(auth.to_string());
            }
        }
        endpoint = endpoint.tls_config(tls).context("配置 TLS 失败")?;
    }
    Ok(endpoint)
}

fn apply_metadata(map: &mut MetadataMap, pairs: &[GrpcMetadataPair]) -> Result<()> {
    for pair in pairs {
        let key = pair.key.trim();
        if key.is_empty() {
            continue;
        }
        let k = AsciiMetadataKey::from_bytes(key.as_bytes())
            .map_err(|e| anyhow!("非法 metadata key「{key}」：{e}"))?;
        let v: AsciiMetadataValue = MetadataValue::try_from(pair.value.as_str())
            .map_err(|e| anyhow!("非法 metadata value「{}」：{e}", pair.value))?;
        map.insert(k, v);
    }
    Ok(())
}

fn metadata_to_headers(map: &MetadataMap) -> Vec<HttpHeader> {
    map.iter()
        .filter_map(|kv| match kv {
            tonic::metadata::KeyAndValueRef::Ascii(k, v) => Some(HttpHeader {
                key: k.as_str().to_string(),
                value: v.to_str().unwrap_or("").to_string(),
            }),
            tonic::metadata::KeyAndValueRef::Binary(k, v) => Some(HttpHeader {
                key: k.as_str().to_string(),
                value: format!(
                    "<binary {} bytes>",
                    v.to_bytes().map(|b| b.len()).unwrap_or(0)
                ),
            }),
        })
        .collect()
}

fn dynamic_message_to_json(message: &DynamicMessage) -> Result<String> {
    let mut serializer = serde_json::Serializer::pretty(Vec::<u8>::new());
    let opts = prost_reflect::SerializeOptions::new()
        .stringify_64_bit_integers(false)
        .use_proto_field_name(false)
        .skip_default_fields(false);
    message
        .serialize_with_options(&mut serializer, &opts)
        .context("序列化响应为 JSON 失败")?;
    let bytes = serializer.into_inner();
    String::from_utf8(bytes).context("响应 JSON 不是有效 UTF-8")
}

/// 动态消息 codec，让 tonic 知道用哪个 MessageDescriptor 来解码响应。
struct DynCodec {
    output: MessageDescriptor,
}

impl Codec for DynCodec {
    type Encode = DynamicMessage;
    type Decode = DynamicMessage;
    type Encoder = DynEncoder;
    type Decoder = DynDecoder;

    fn encoder(&mut self) -> Self::Encoder {
        DynEncoder
    }

    fn decoder(&mut self) -> Self::Decoder {
        DynDecoder {
            output: self.output.clone(),
        }
    }
}

struct DynEncoder;

impl Encoder for DynEncoder {
    type Item = DynamicMessage;
    type Error = Status;

    fn encode(&mut self, item: Self::Item, dst: &mut EncodeBuf<'_>) -> Result<(), Self::Error> {
        let mut buf = Vec::with_capacity(item.encoded_len());
        item.encode(&mut buf)
            .map_err(|e| Status::internal(format!("encode error: {e}")))?;
        dst.put_slice(&buf);
        Ok(())
    }
}

struct DynDecoder {
    output: MessageDescriptor,
}

impl Decoder for DynDecoder {
    type Item = DynamicMessage;
    type Error = Status;

    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<Self::Item>, Self::Error> {
        let remaining = src.remaining();
        if remaining == 0 {
            return Ok(Some(DynamicMessage::new(self.output.clone())));
        }
        let bytes = src.copy_to_bytes(remaining);
        let msg = DynamicMessage::decode(self.output.clone(), bytes.as_ref())
            .map_err(|e| Status::internal(format!("decode error: {e}")))?;
        Ok(Some(msg))
    }
}
