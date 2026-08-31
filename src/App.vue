<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import postboyLogo from "./assets/postboy-logo.svg";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";

type HttpMethod = "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
type BodyType = "none" | "form-data" | "x-www-form-urlencoded" | "raw" | "binary";
type RawLang = "Text" | "JSON" | "XML" | "HTML" | "JavaScript";
type Theme = "dark" | "light";
type EnvKind = "global" | "dev" | "stg" | "prd" | "custom";
type SidebarTarget = "collections" | "environments" | "history" | "flows";
type RequestTab = "params" | "headers" | "body" | "auth" | "tests" | "message" | "metadata";
type ResponseSubTab = "body" | "headers" | "cookies" | "tests";
type ResponseFormat = "pretty" | "raw" | "preview";
type ResponseLang = "JSON" | "XML" | "HTML" | "Text";
type RequestKind = "http" | "grpc";

interface KV {
  enabled: boolean;
  key: string;
  value: string;
  description: string;
}

type BodyFormFieldType = "text" | "file";

interface BodyFormRow extends KV {
  fieldType: BodyFormFieldType;
  filePath: string;
  fileName: string;
}

interface GrpcConfig {
  target: string;
  protoPath: string;
  importPaths: string;
  service: string;
  method: string;
  message: string;
  metadata: KV[];
  useTls: boolean;
  authority: string;
}

interface ProtoMethodDescriptor {
  name: string;
  input_type: string;
  output_type: string;
  client_streaming: boolean;
  server_streaming: boolean;
  request_template: string;
}

interface ProtoServiceDescriptor {
  name: string;
  methods: ProtoMethodDescriptor[];
}

interface ProtoParseEntry {
  loading: boolean;
  services: ProtoServiceDescriptor[] | null;
  error: string;
}

interface RequestRecord {
  id: string;
  folderId: string;
  name: string;
  kind: RequestKind;
  method: HttpMethod;
  url: string;
  params: KV[];
  headers: KV[];
  bodyType: BodyType;
  rawLang: RawLang;
  body: string;
  bodyRows: BodyFormRow[];
  binaryPath: string;
  grpc: GrpcConfig;
  tests: string;
}

interface FolderRecord {
  id: string;
  name: string;
  expanded: boolean;
}

interface EnvRecord {
  id: string;
  name: string;
  kind: EnvKind;
  variables: KV[];
  headers: KV[];
}

interface HistorySnapshot {
  kind: RequestKind;
  name?: string;
  method: HttpMethod;
  url: string;
  params: KV[];
  headers: KV[];
  bodyType: BodyType;
  rawLang: RawLang;
  body: string;
  bodyRows?: BodyFormRow[];
  binaryPath?: string;
  grpc?: GrpcConfig;
}

interface TestResult {
  name: string;
  passed: boolean;
  error: string;
  durationMs: number;
}

interface HistoryResponse {
  status: number | null;
  headers: ResponseHeader[];
  body: string;
  elapsedMs: number | null;
  sizeBytes: number | null;
  lang?: ResponseLang;
  error?: string;
  tests?: TestResult[];
}

interface HistoryEntry {
  id: string;
  method: HttpMethod;
  url: string;
  status: number | null;
  timestamp: number;
  snapshot?: HistorySnapshot;
  response?: HistoryResponse;
}

interface ResponseHeader {
  key: string;
  value: string;
}

interface HttpResponsePayload {
  status: number;
  headers: ResponseHeader[];
  body: string;
  elapsed_ms: number;
  size_bytes: number;
}

interface ParsedCurl {
  method: HttpMethod;
  url: string;
  headers: { key: string; value: string }[];
  body: string;
  bodyType: BodyType;
  rawLang: RawLang;
}

interface PersistedState {
  folders: FolderRecord[];
  requests: RequestRecord[];
  environments: EnvRecord[];
  activeEnvId: string;
  selectedEnvId: string;
  openTabIds: string[];
  activeTabId: string;
  history: HistoryEntry[];
  flows?: FlowRecord[];
  activeFlowId?: string;
  sidebarWidth?: number;
  requestPaneHeight?: number;
  theme?: Theme;
}

type FlowMode = "sequence" | "concurrent" | "data-driven";

interface FlowStep {
  id: string;
  requestId: string;
  name?: string;
  extractScript: string;
  skipIf?: string;
}

interface FlowDataRow {
  id: string;
  values: Record<string, string>;
}

interface FlowRecord {
  id: string;
  name: string;
  mode: FlowMode;
  steps: FlowStep[];
  iterations: number;
  concurrency: number;
  dataColumns: string[];
  dataRows: FlowDataRow[];
  createdAt: number;
  updatedAt: number;
}

interface FlowStepResult {
  stepId: string;
  requestId: string;
  requestName: string;
  ok: boolean;
  status?: number;
  elapsedMs?: number;
  sizeBytes?: number;
  error?: string;
  extractError?: string;
  response?: HistoryResponse;
}

interface FlowIterationResult {
  iteration: number;
  startedAt: number;
  finishedAt: number;
  totalMs: number;
  steps: FlowStepResult[];
  finalVars: Record<string, string>;
  rowLabel?: string;
}

interface FlowRunSummary {
  flowId: string;
  flowName: string;
  mode: FlowMode;
  iterations: FlowIterationResult[];
  totalMs: number;
  passed: number;
  failed: number;
  startedAt: number;
  finishedAt: number;
}

const SIDEBAR_MIN = 140;
const SIDEBAR_MAX = 560;
const SIDEBAR_COMPACT_THRESHOLD = 240;
const REQUEST_PANE_MIN = 180;
const REQUEST_PANE_MAX = 720;
const HISTORY_MAX_ENTRIES = 100;

function clamp(value: number, min: number, max: number): number {
  if (!Number.isFinite(value)) return min;
  return Math.min(max, Math.max(min, value));
}

const STORAGE_KEY = "postboy_state_v2";
const REQUEST_TIMEOUT_MS = 120_000;
const REQUEST_TIMEOUT_MESSAGE = "请求已超时（超过 120 秒）";
const BODY_TYPES: BodyType[] = [
  "none",
  "form-data",
  "x-www-form-urlencoded",
  "raw",
  "binary",
];

const folders = ref<FolderRecord[]>([]);
const requests = ref<RequestRecord[]>([]);
const environments = ref<EnvRecord[]>([]);
const activeEnvId = ref("");
const selectedEnvId = ref("");
const openTabIds = ref<string[]>([]);
const activeTabId = ref("");
const selectedFolderId = ref("");
const history = ref<HistoryEntry[]>([]);
const flows = ref<FlowRecord[]>([]);
const activeFlowId = ref("");
const flowSearchQuery = ref("");
const flowRunSummary = ref<FlowRunSummary | null>(null);
const flowRunning = ref(false);
const flowExpandedIterations = ref<Record<number, boolean>>({});
// In-memory cache of the most recent response per request tab. Keyed by
// `RequestRecord.id`. Used to restore the response panel when the user
// switches tabs in the collection sidebar so the previously shown
// status/headers/body don't disappear.
const lastResponses = ref<Record<string, HistoryResponse>>({});

const sidebarTarget = ref<SidebarTarget>("collections");
const requestTab = ref<RequestTab>("params");
const responseSubTab = ref<ResponseSubTab>("body");
const responseFormat = ref<ResponseFormat>("pretty");
const responseLang = ref<ResponseLang>("JSON");

const sendingRequestIds = ref<Record<string, true>>({});
const errorMessage = ref("");
const responseStatus = ref<number | null>(null);
const responseTimeMs = ref<number | null>(null);
const responseSize = ref<number | null>(null);
const responseHeaders = ref<ResponseHeader[]>([]);
const responseBody = ref("");
const responseTests = ref<TestResult[]>([]);
const bodyJsonCollapsed = ref<Record<string, string[]>>({});
const responseJsonCollapsed = ref<string[]>([]);
let sendRunSeq = 0;
const activeSendRunIds: Record<string, number> = {};
const sendingTimeouts: Record<string, ReturnType<typeof setTimeout>> = {};

const storagePathLabel = ref("~/.postboy/postboy.db");
const searchQuery = ref("");
const envSearchQuery = ref("");
const historySearchQuery = ref("");

const sidebarWidth = ref(280);
const requestPaneHeight = ref(360);
const theme = ref<Theme>("dark");
const isLightTheme = computed(() => theme.value === "light");
const appTitle = ref<"Postboy" | "Postgirl">("Postboy");

function applyTheme() {
  if (typeof document !== "undefined") {
    document.documentElement.dataset.theme = theme.value;
  }
}

function toggleTheme() {
  theme.value = theme.value === "dark" ? "light" : "dark";
}

function toggleAppTitle() {
  appTitle.value = appTitle.value === "Postboy" ? "Postgirl" : "Postboy";
  document.title = appTitle.value;
  void getCurrentWindow().setTitle(appTitle.value);
}

watch(theme, applyTheme, { immediate: true });

const openDropdown = ref<string>("");
function toggleDropdown(name: string) {
  openDropdown.value = openDropdown.value === name ? "" : name;
}
function closeDropdown() {
  openDropdown.value = "";
}
function onGlobalClick(event: MouseEvent) {
  const target = event.target as HTMLElement | null;
  if (!target) {
    closeDropdown();
    return;
  }
  if (
    !target.closest(".custom-select") &&
    !target.closest(".send-group") &&
    !target.closest(".new-split")
  ) {
    closeDropdown();
  }
}

const showImportModal = ref(false);
const importCurlText = ref("");
const importParsed = ref<ParsedCurl | null>(null);
const importError = ref("");
const importTargetFolderId = ref<string>("__new__");
const importNewFolderName = ref("");
const importRequestName = ref("");
const importSource = ref<"curl" | "openapi" | "har" | "postman">("curl");

interface PromptState {
  visible: boolean;
  title: string;
  label: string;
  placeholder: string;
  value: string;
  confirmText: string;
  cancelText: string;
  resolve: ((value: string | null) => void) | null;
}
const promptState = ref<PromptState>({
  visible: false,
  title: "",
  label: "",
  placeholder: "",
  value: "",
  confirmText: "确定",
  cancelText: "取消",
  resolve: null,
});
const promptInputEl = ref<HTMLInputElement | null>(null);

function openPrompt(opts: {
  title: string;
  label?: string;
  placeholder?: string;
  defaultValue?: string;
  confirmText?: string;
  cancelText?: string;
}): Promise<string | null> {
  return new Promise((resolve) => {
    promptState.value = {
      visible: true,
      title: opts.title,
      label: opts.label ?? "",
      placeholder: opts.placeholder ?? "",
      value: opts.defaultValue ?? "",
      confirmText: opts.confirmText ?? "确定",
      cancelText: opts.cancelText ?? "取消",
      resolve,
    };
    setTimeout(() => promptInputEl.value?.focus(), 30);
  });
}

function confirmPrompt() {
  const v = promptState.value.value.trim();
  const r = promptState.value.resolve;
  promptState.value.visible = false;
  promptState.value.resolve = null;
  r?.(v ? v : null);
}

function cancelPrompt() {
  const r = promptState.value.resolve;
  promptState.value.visible = false;
  promptState.value.resolve = null;
  r?.(null);
}

interface ConfirmState {
  visible: boolean;
  title: string;
  message: string;
  confirmText: string;
  cancelText: string;
  danger: boolean;
  resolve: ((ok: boolean) => void) | null;
}
const confirmState = ref<ConfirmState>({
  visible: false,
  title: "",
  message: "",
  confirmText: "确定",
  cancelText: "取消",
  danger: false,
  resolve: null,
});

function openConfirm(opts: {
  title: string;
  message: string;
  confirmText?: string;
  cancelText?: string;
  danger?: boolean;
}): Promise<boolean> {
  return new Promise((resolve) => {
    confirmState.value = {
      visible: true,
      title: opts.title,
      message: opts.message,
      confirmText: opts.confirmText ?? "确定",
      cancelText: opts.cancelText ?? "取消",
      danger: !!opts.danger,
      resolve,
    };
  });
}

function answerConfirm(ok: boolean) {
  const r = confirmState.value.resolve;
  confirmState.value.visible = false;
  confirmState.value.resolve = null;
  r?.(ok);
}

let hydrated = false;

function uuid(): string {
  return crypto.randomUUID();
}

function emptyRow(): KV {
  return { enabled: true, key: "", value: "", description: "" };
}

function emptyBodyRow(): BodyFormRow {
  return {
    ...emptyRow(),
    fieldType: "text",
    filePath: "",
    fileName: "",
  };
}

function makeFolder(name: string): FolderRecord {
  return { id: uuid(), name, expanded: true };
}

function makeGrpcConfig(): GrpcConfig {
  return {
    target: "localhost:50051",
    protoPath: "",
    importPaths: "",
    service: "",
    method: "",
    message: "{}",
    metadata: [emptyRow()],
    useTls: false,
    authority: "",
  };
}

function makeRequest(
  folderId: string,
  name?: string,
  kind: RequestKind = "http",
): RequestRecord {
  const defaultName =
    kind === "grpc" ? name ?? "新建 gRPC 请求" : name ?? "新建请求";
  return {
    id: uuid(),
    folderId,
    name: defaultName,
    kind,
    method: "GET",
    url: "",
    params: [emptyRow()],
    headers: [emptyRow()],
    bodyType: "none",
    rawLang: "JSON",
    body: "",
    bodyRows: [emptyBodyRow()],
    binaryPath: "",
    grpc: makeGrpcConfig(),
    tests: "",
  };
}

function defaultEnvironments(): EnvRecord[] {
  return [
    {
      id: uuid(),
      name: "Globals（全局）",
      kind: "global",
      variables: [
        { enabled: true, key: "token", value: "your-token-here", description: "全局令牌" },
        emptyRow(),
      ],
      headers: [emptyRow()],
    },
    {
      id: uuid(),
      name: "Dev",
      kind: "dev",
      variables: [
        { enabled: true, key: "baseUrl", value: "http://127.0.0.1:3000", description: "" },
        emptyRow(),
      ],
      headers: [emptyRow()],
    },
    {
      id: uuid(),
      name: "Staging",
      kind: "stg",
      variables: [
        { enabled: true, key: "baseUrl", value: "https://stg.example.com", description: "" },
        emptyRow(),
      ],
      headers: [emptyRow()],
    },
    {
      id: uuid(),
      name: "Production",
      kind: "prd",
      variables: [
        { enabled: true, key: "baseUrl", value: "https://api.example.com", description: "" },
        emptyRow(),
      ],
      headers: [emptyRow()],
    },
  ];
}

function defaultState(): PersistedState {
  const folder = makeFolder("用户中心 API");
  const exampleRequest: RequestRecord = {
    id: uuid(),
    folderId: folder.id,
    name: "获取用户信息",
    kind: "http",
    method: "GET",
    url: "{{baseUrl}}/api/users/1024",
    params: [
      { enabled: true, key: "include", value: "profile,settings", description: "包含的关联数据" },
      { enabled: false, key: "lang", value: "zh-CN", description: "" },
      emptyRow(),
    ],
    headers: [
      { enabled: true, key: "Content-Type", value: "application/json", description: "" },
      { enabled: true, key: "Authorization", value: "Bearer {{token}}", description: "" },
      emptyRow(),
    ],
    bodyType: "none",
    rawLang: "JSON",
    body: "",
    bodyRows: [emptyBodyRow()],
    binaryPath: "",
    grpc: makeGrpcConfig(),
    tests: "",
  };
  const envs = defaultEnvironments();
  const dev = envs.find((env) => env.kind === "dev")!;
  const global = envs.find((env) => env.kind === "global")!;
  return {
    folders: [folder],
    requests: [exampleRequest],
    environments: envs,
    activeEnvId: dev.id,
    selectedEnvId: global.id,
    openTabIds: [exampleRequest.id],
    activeTabId: exampleRequest.id,
    history: [],
  };
}

function normalizeRows(rows: unknown): KV[] {
  if (!Array.isArray(rows)) return [emptyRow()];
  const list = rows
    .filter((row) => row && typeof row === "object")
    .map((row) => {
      const r = row as Partial<KV>;
      return {
        enabled: r.enabled !== false,
        key: String(r.key ?? ""),
        value: String(r.value ?? ""),
        description: String(r.description ?? ""),
      };
    });
  if (!list.length) return [emptyRow()];
  if (list[list.length - 1].key || list[list.length - 1].value) {
    list.push(emptyRow());
  }
  return list;
}

function normalizeBodyRows(rows: unknown): BodyFormRow[] {
  if (!Array.isArray(rows)) return [emptyBodyRow()];
  const list = rows
    .filter((row) => row && typeof row === "object")
    .map((row) => {
      const r = row as Partial<BodyFormRow>;
      return {
        enabled: r.enabled !== false,
        key: String(r.key ?? ""),
        value: String(r.value ?? ""),
        description: String(r.description ?? ""),
        fieldType: (r.fieldType === "file" ? "file" : "text") as BodyFormFieldType,
        filePath: String(r.filePath ?? ""),
        fileName: String(r.fileName ?? ""),
      };
    });
  if (!list.length) return [emptyBodyRow()];
  const last = list[list.length - 1];
  if (last.key || last.value || last.filePath) {
    list.push(emptyBodyRow());
  }
  return list;
}

function normalizeGrpc(raw: any): GrpcConfig {
  const base = makeGrpcConfig();
  if (!raw || typeof raw !== "object") return base;
  return {
    target: String(raw.target ?? base.target),
    protoPath: String(raw.protoPath ?? ""),
    importPaths: String(raw.importPaths ?? ""),
    service: String(raw.service ?? ""),
    method: String(raw.method ?? ""),
    message: String(raw.message ?? "{}"),
    metadata: normalizeRows(raw.metadata),
    useTls: !!raw.useTls,
    authority: String(raw.authority ?? ""),
  };
}

function normalizeRequest(raw: any): RequestRecord {
  const kind: RequestKind = raw?.kind === "grpc" ? "grpc" : "http";
  const bodyType = (raw?.bodyType ?? "none") as BodyType;
  const body = String(raw?.body ?? "");
  return {
    id: String(raw?.id ?? uuid()),
    folderId: String(raw?.folderId ?? ""),
    name: String(raw?.name ?? "未命名请求"),
    kind,
    method: (String(raw?.method ?? "GET").toUpperCase()) as HttpMethod,
    url: String(raw?.url ?? ""),
    params: normalizeRows(raw?.params),
    headers: normalizeRows(raw?.headers),
    bodyType,
    rawLang: (raw?.rawLang ?? "JSON") as RawLang,
    body,
    bodyRows: Array.isArray(raw?.bodyRows)
      ? normalizeBodyRows(raw.bodyRows)
      : bodyType === "x-www-form-urlencoded"
        ? bodyRowsFromUrlEncoded(body)
        : [emptyBodyRow()],
    binaryPath: String(raw?.binaryPath ?? ""),
    grpc: normalizeGrpc(raw?.grpc),
    tests: String(raw?.tests ?? ""),
  };
}

function normalizeEnv(raw: any): EnvRecord {
  return {
    id: String(raw?.id ?? uuid()),
    name: String(raw?.name ?? "未命名环境"),
    kind: (raw?.kind ?? "custom") as EnvKind,
    variables: normalizeRows(raw?.variables),
    headers: normalizeRows(raw?.headers),
  };
}

function normalizeFlow(raw: any): FlowRecord {
  const modeRaw = String(raw?.mode ?? "sequence");
  const mode: FlowMode =
    modeRaw === "concurrent" || modeRaw === "data-driven" ? (modeRaw as FlowMode) : "sequence";
  const steps: FlowStep[] = Array.isArray(raw?.steps)
    ? raw.steps.map((s: any) => ({
        id: String(s?.id ?? uuid()),
        requestId: String(s?.requestId ?? ""),
        name: s?.name != null ? String(s.name) : undefined,
        extractScript: String(s?.extractScript ?? ""),
        skipIf: s?.skipIf != null ? String(s.skipIf) : undefined,
      }))
    : [];
  const dataColumns: string[] = Array.isArray(raw?.dataColumns)
    ? raw.dataColumns.map((c: any) => String(c ?? ""))
    : [];
  const dataRows: FlowDataRow[] = Array.isArray(raw?.dataRows)
    ? raw.dataRows.map((r: any) => ({
        id: String(r?.id ?? uuid()),
        values:
          r?.values && typeof r.values === "object"
            ? Object.fromEntries(
                Object.entries(r.values).map(([k, v]) => [String(k), String(v ?? "")]),
              )
            : {},
      }))
    : [];
  const now = Date.now();
  return {
    id: String(raw?.id ?? uuid()),
    name: String(raw?.name ?? "未命名流程"),
    mode,
    steps,
    iterations: clamp(Number(raw?.iterations ?? 1), 1, 100000),
    concurrency: clamp(Number(raw?.concurrency ?? 1), 1, 500),
    dataColumns,
    dataRows,
    createdAt: Number(raw?.createdAt ?? now),
    updatedAt: Number(raw?.updatedAt ?? now),
  };
}

async function loadStateRaw(): Promise<string> {
  try {
    return await invoke<string>("load_state");
  } catch {
    return localStorage.getItem(STORAGE_KEY) ?? "";
  }
}

async function fetchStoragePath(): Promise<string> {
  try {
    return await invoke<string>("storage_path");
  } catch {
    return "~/.postboy/postboy.db";
  }
}

async function recordHistoryRemote(entry: {
  request_id?: string;
  method: HttpMethod;
  url: string;
  status: number | null;
  elapsed_ms?: number | null;
  size_bytes?: number | null;
  error?: string | null;
  snapshot?: HistorySnapshot;
  response?: HistoryResponse;
}): Promise<void> {
  try {
    await invoke("record_history", { entry });
  } catch {
    // ignore – in-memory copy is still fine
  }
}

function appendHistoryEntry(entry: HistoryEntry) {
  history.value = [entry, ...history.value].slice(0, HISTORY_MAX_ENTRIES);
}

function cloneKvList(list: KV[] | undefined): KV[] {
  if (!Array.isArray(list)) return [];
  return list.map((row) => ({ ...row }));
}

function cloneBodyRows(list: BodyFormRow[] | undefined): BodyFormRow[] {
  if (!Array.isArray(list)) return [];
  return list.map((row) => ({ ...row }));
}

function snapshotFromRequest(req: RequestRecord): HistorySnapshot {
  return {
    kind: req.kind,
    name: req.name,
    method: req.method,
    url: req.url,
    params: cloneKvList(req.params),
    headers: cloneKvList(req.headers),
    bodyType: req.bodyType,
    rawLang: req.rawLang,
    body: req.body ?? "",
    bodyRows: cloneBodyRows(req.bodyRows),
    binaryPath: req.binaryPath ?? "",
    grpc: req.grpc
      ? { ...req.grpc, metadata: cloneKvList(req.grpc.metadata) }
      : undefined,
  };
}

async function clearHistoryRemote(): Promise<void> {
  try {
    await invoke("clear_history");
  } catch {
    // ignore
  }
}

let saveTimer: number | null = null;
function schedulePersist() {
  if (!hydrated) return;
  if (saveTimer !== null) window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(persistNow, 250);
}

async function persistNow() {
  saveTimer = null;
  const payload: Omit<PersistedState, "history"> = {
    folders: folders.value,
    requests: requests.value,
    environments: environments.value,
    activeEnvId: activeEnvId.value,
    selectedEnvId: selectedEnvId.value,
    openTabIds: openTabIds.value,
    activeTabId: activeTabId.value,
    flows: flows.value,
    activeFlowId: activeFlowId.value,
    sidebarWidth: Math.round(sidebarWidth.value),
    requestPaneHeight: Math.round(requestPaneHeight.value),
    theme: theme.value,
  };
  const serialized = JSON.stringify(payload);
  try {
    await invoke("save_state", { state: serialized });
  } catch {
    try {
      localStorage.setItem(STORAGE_KEY, serialized);
    } catch {
      // ignore quota errors
    }
  }
}

async function hydrate() {
  const raw = await loadStateRaw();
  let parsed: PersistedState | null = null;
  if (raw) {
    try {
      parsed = JSON.parse(raw) as PersistedState;
    } catch {
      parsed = null;
    }
  }
  const state = parsed ?? defaultState();

  folders.value = (state.folders ?? []).map((f) => ({
    id: String(f.id ?? uuid()),
    name: String(f.name ?? "未命名"),
    expanded: f.expanded !== false,
  }));
  requests.value = (state.requests ?? []).map(normalizeRequest);
  const rawEnvs = Array.isArray(state.environments) && state.environments.length
    ? state.environments
    : defaultEnvironments();
  environments.value = rawEnvs.map(normalizeEnv);
  if (!environments.value.some((e) => e.kind === "global")) {
    const globalDefault = defaultEnvironments().find((e) => e.kind === "global")!;
    environments.value = [globalDefault, ...environments.value];
  }

  const validRequestIds = new Set(requests.value.map((r) => r.id));
  const validEnvIds = new Set(environments.value.map((e) => e.id));

  const dev = environments.value.find((e) => e.kind === "dev");
  activeEnvId.value = validEnvIds.has(state.activeEnvId)
    ? state.activeEnvId
    : dev?.id ?? environments.value[0]?.id ?? "";

  const globalEnvId = environments.value.find((e) => e.kind === "global")?.id ?? "";
  selectedEnvId.value = validEnvIds.has(state.selectedEnvId)
    ? state.selectedEnvId
    : globalEnvId;

  openTabIds.value = (state.openTabIds ?? []).filter((id) => validRequestIds.has(id));
  activeTabId.value = openTabIds.value.includes(state.activeTabId)
    ? state.activeTabId
    : openTabIds.value[0] ?? "";

  const activeOwner = requests.value.find((r) => r.id === activeTabId.value);
  selectedFolderId.value = activeOwner?.folderId ?? folders.value[0]?.id ?? "";

  history.value = (state.history ?? []).slice(0, HISTORY_MAX_ENTRIES);

  flows.value = Array.isArray(state.flows) ? state.flows.map(normalizeFlow) : [];
  activeFlowId.value = flows.value.some((f) => f.id === state.activeFlowId)
    ? String(state.activeFlowId)
    : "";

  if (typeof state.sidebarWidth === "number") {
    sidebarWidth.value = clamp(state.sidebarWidth, SIDEBAR_MIN, SIDEBAR_MAX);
  }
  if (typeof state.requestPaneHeight === "number") {
    requestPaneHeight.value = clamp(
      state.requestPaneHeight,
      REQUEST_PANE_MIN,
      REQUEST_PANE_MAX,
    );
  }
  if (state.theme === "light" || state.theme === "dark") {
    theme.value = state.theme;
  }

  storagePathLabel.value = await fetchStoragePath();

  hydrated = true;
}

const activeRequest = computed<RequestRecord | null>(
  () => requests.value.find((r) => r.id === activeTabId.value) ?? null,
);
const isActiveRequestSending = computed(
  () => !!activeRequest.value && isRequestSending(activeRequest.value.id),
);

const openTabs = computed(() =>
  openTabIds.value
    .map((id) => requests.value.find((r) => r.id === id))
    .filter((r): r is RequestRecord => Boolean(r)),
);

const activeFolderName = computed(() => {
  const req = activeRequest.value;
  if (!req) return "";
  return folders.value.find((f) => f.id === req.folderId)?.name ?? "";
});

const activeEnv = computed(() =>
  environments.value.find((e) => e.id === activeEnvId.value),
);
const activeEnvLabel = computed(() => {
  const env = activeEnv.value;
  if (!env) return "无";
  return env.name.split("（")[0];
});
const activeEnvDotKind = computed(() => activeEnv.value?.kind ?? "dev");

const globalEnv = computed(() =>
  environments.value.find((e) => e.kind === "global"),
);

const selectedEnv = computed(() =>
  environments.value.find((e) => e.id === selectedEnvId.value),
);

function envLookup(name: string): string | null {
  const fromActive = activeEnv.value?.variables.find(
    (v) => v.enabled && v.key === name,
  );
  if (fromActive && fromActive.value) return fromActive.value;
  const fromGlobal = globalEnv.value?.variables.find(
    (v) => v.enabled && v.key === name,
  );
  if (fromGlobal && fromGlobal.value) return fromGlobal.value;
  return null;
}

function substituteVars(text: string, extraVars?: Record<string, string>): string {
  return text.replace(/\{\{\s*([\w-]+)\s*\}\}/g, (match, name) => {
    const key = String(name);
    if (extraVars && Object.prototype.hasOwnProperty.call(extraVars, key)) {
      return extraVars[key];
    }
    const v = envLookup(key);
    return v == null ? match : v;
  });
}

function methodKey(method: HttpMethod): string {
  return method.toLowerCase();
}

function methodTagText(method: HttpMethod): string {
  return method === "DELETE" ? "DEL" : method;
}

function requestTagKey(req: { kind: RequestKind; method: HttpMethod }): string {
  return req.kind === "grpc" ? "grpc" : methodKey(req.method);
}

function requestTagText(req: { kind: RequestKind; method: HttpMethod }): string {
  return req.kind === "grpc" ? "gRPC" : methodTagText(req.method);
}

function langTagShort(l: string): string {
  if (l === "Text") return "TEXT";
  if (l === "JavaScript") return "JS";
  return l.toUpperCase();
}

function langTagClass(l: string): string {
  if (l === "Text") return "text";
  if (l === "JavaScript") return "js";
  return l.toLowerCase();
}

const compactSidebar = computed(
  () => sidebarWidth.value < SIDEBAR_COMPACT_THRESHOLD,
);

const searchStyle = computed(
  () =>
    ({ display: compactSidebar.value ? "none" : "flex" }) as Record<
      string,
      string
    >,
);

const importParsedHeaderKeys = computed(() => {
  const list = importParsed.value?.headers ?? [];
  return list.map((h) => h.key);
});

const importParsedBodySize = computed(() => {
  const body = importParsed.value?.body ?? "";
  if (!body) return "";
  const bytes = new Blob([body]).size;
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`;
});

const importParsedBodyTagClass = computed(() => {
  if (!importParsed.value || !importParsed.value.body) return "";
  if (importParsed.value.bodyType === "x-www-form-urlencoded") return "form";
  return langTagClass(importParsed.value.rawLang);
});

const importParsedBodyTagText = computed(() => {
  if (!importParsed.value || !importParsed.value.body) return "";
  if (importParsed.value.bodyType === "x-www-form-urlencoded") return "FORM";
  return langTagShort(importParsed.value.rawLang);
});

const importTargetFolderName = computed(() => {
  const id = importTargetFolderId.value;
  if (id === "__new__") return importNewFolderName.value.trim() || "新建集合…";
  return folders.value.find((f) => f.id === id)?.name ?? "选择集合";
});

const filteredFolders = computed(() => {
  const q = searchQuery.value.trim().toLowerCase();
  if (!q) return folders.value;
  return folders.value.filter((folder) => {
    if (folder.name.toLowerCase().includes(q)) return true;
    return requests.value.some(
      (r) =>
        r.folderId === folder.id &&
        (r.name.toLowerCase().includes(q) || r.url.toLowerCase().includes(q)),
    );
  });
});

function requestsInFolder(folderId: string): RequestRecord[] {
  const list = requests.value.filter((r) => r.folderId === folderId);
  const q = searchQuery.value.trim().toLowerCase();
  if (!q) return list;
  const folder = folders.value.find((f) => f.id === folderId);
  if (folder?.name.toLowerCase().includes(q)) return list;
  return list.filter(
    (r) => r.name.toLowerCase().includes(q) || r.url.toLowerCase().includes(q),
  );
}

const filteredEnvs = computed(() => {
  const q = envSearchQuery.value.trim().toLowerCase();
  if (!q) return environments.value;
  return environments.value.filter((env) => env.name.toLowerCase().includes(q));
});

const filteredHistory = computed(() => {
  const q = historySearchQuery.value.trim().toLowerCase();
  if (!q) return history.value;
  return history.value.filter((entry) => {
    if (entry.url?.toLowerCase().includes(q)) return true;
    if (entry.method?.toLowerCase().includes(q)) return true;
    if (String(entry.status ?? "").includes(q)) return true;
    return false;
  });
});

const paramsCount = computed(
  () =>
    activeRequest.value?.params.filter((p) => p.enabled && p.key.trim()).length ?? 0,
);
const headersCount = computed(
  () =>
    activeRequest.value?.headers.filter((h) => h.enabled && h.key.trim()).length ?? 0,
);
const grpcMetaCount = computed(
  () =>
    activeRequest.value?.grpc.metadata.filter((m) => m.enabled && m.key.trim()).length ?? 0,
);
const testsLineCount = computed(() => {
  const script = activeRequest.value?.tests?.trim();
  if (!script) return 0;
  return (script.match(/pm\.test\s*\(/g) ?? []).length;
});
const testsPassedCount = computed(() => responseTests.value.filter((t) => t.passed).length);
const testsFailedCount = computed(() => responseTests.value.filter((t) => !t.passed).length);

const statusClass = computed(() => {
  const s = responseStatus.value;
  if (s == null) return "";
  return s >= 200 && s < 400 ? "ok" : "err";
});

function statusText(status: number): string {
  const map: Record<number, string> = {
    200: "OK",
    201: "Created",
    202: "Accepted",
    204: "No Content",
    301: "Moved Permanently",
    302: "Found",
    304: "Not Modified",
    400: "Bad Request",
    401: "Unauthorized",
    403: "Forbidden",
    404: "Not Found",
    405: "Method Not Allowed",
    409: "Conflict",
    422: "Unprocessable",
    429: "Too Many",
    500: "Server Error",
    502: "Bad Gateway",
    503: "Unavailable",
    504: "Timeout",
  };
  return map[status] ?? "";
}

const sizeLabel = computed(() => {
  const bytes = responseSize.value;
  if (bytes == null) return "";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`;
});

function ensureTrailingRow(rows: KV[]) {
  if (!rows.length) {
    rows.push(emptyRow());
    return;
  }
  const last = rows[rows.length - 1];
  if (last.key || last.value) rows.push(emptyRow());
}

function removeRow(rows: KV[], index: number) {
  rows.splice(index, 1);
  if (!rows.length) rows.push(emptyRow());
}

function ensureTrailingBodyRow(rows: BodyFormRow[]) {
  if (!rows.length) {
    rows.push(emptyBodyRow());
    return;
  }
  const last = rows[rows.length - 1];
  if (last.key || last.value || last.filePath) rows.push(emptyBodyRow());
}

function removeBodyRow(rows: BodyFormRow[], index: number) {
  rows.splice(index, 1);
  if (!rows.length) rows.push(emptyBodyRow());
}

function filenameFromPath(path: string): string {
  const idx = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return idx >= 0 ? path.slice(idx + 1) : path;
}

function selectFolder(folder: FolderRecord) {
  selectedFolderId.value = folder.id;
}

function toggleFolder(folder: FolderRecord) {
  folder.expanded = !folder.expanded;
  selectFolder(folder);
}

async function addFolder() {
  const name = await openPrompt({
    title: "新建集合",
    label: "集合名称",
    placeholder: "例如：用户服务",
    confirmText: "创建",
  });
  if (!name) return;
  const folder = makeFolder(name);
  folders.value.push(folder);
  selectedFolderId.value = folder.id;
}

async function deleteFolder(folder: FolderRecord) {
  const ok = await openConfirm({
    title: "删除集合",
    message: `确定删除集合「${folder.name}」及其所有请求吗？此操作不可撤销。`,
    confirmText: "删除",
    danger: true,
  });
  if (!ok) return;
  const childIds = requests.value
    .filter((r) => r.folderId === folder.id)
    .map((r) => r.id);
  requests.value = requests.value.filter((r) => r.folderId !== folder.id);
  openTabIds.value = openTabIds.value.filter((id) => !childIds.includes(id));
  childIds.forEach(forgetResponse);
  if (childIds.includes(activeTabId.value)) {
    activeTabId.value = openTabIds.value[0] ?? "";
  }
  folders.value = folders.value.filter((f) => f.id !== folder.id);
  if (selectedFolderId.value === folder.id) {
    selectedFolderId.value = folders.value[0]?.id ?? "";
  }
}

function addRequestToFolder(folder: FolderRecord, kind: RequestKind = "http") {
  folder.expanded = true;
  selectedFolderId.value = folder.id;
  const record = makeRequest(folder.id, undefined, kind);
  requests.value.push(record);
  openTab(record.id);
}

function resolveTargetFolder(): FolderRecord {
  const selected = folders.value.find((f) => f.id === selectedFolderId.value);
  if (selected) return selected;
  const active = activeRequest.value;
  if (active) {
    const owner = folders.value.find((f) => f.id === active.folderId);
    if (owner) return owner;
  }
  if (folders.value.length) return folders.value[0];
  const created = makeFolder("新集合");
  folders.value.push(created);
  selectedFolderId.value = created.id;
  return created;
}

async function deleteRequest(req: RequestRecord) {
  const ok = await openConfirm({
    title: "删除请求",
    message: `确定删除请求「${req.name}」吗？`,
    confirmText: "删除",
    danger: true,
  });
  if (!ok) return;
  requests.value = requests.value.filter((r) => r.id !== req.id);
  openTabIds.value = openTabIds.value.filter((id) => id !== req.id);
  forgetResponse(req.id);
  if (activeTabId.value === req.id) {
    activeTabId.value = openTabIds.value[0] ?? "";
  }
}

function applyResponseSnap(resp: HistoryResponse | undefined) {
  if (!resp || typeof resp !== "object") {
    responseStatus.value = null;
    responseTimeMs.value = null;
    responseSize.value = null;
    responseHeaders.value = [];
    responseBody.value = "";
    responseTests.value = [];
    errorMessage.value = "";
    return;
  }
  responseStatus.value = typeof resp.status === "number" ? resp.status : null;
  responseTimeMs.value = typeof resp.elapsedMs === "number" ? resp.elapsedMs : null;
  responseSize.value = typeof resp.sizeBytes === "number" ? resp.sizeBytes : null;
  responseHeaders.value = Array.isArray(resp.headers)
    ? resp.headers.map((h) => ({ key: String(h.key ?? ""), value: String(h.value ?? "") }))
    : [];
  responseBody.value = typeof resp.body === "string" ? resp.body : "";
  responseTests.value = Array.isArray(resp.tests) ? resp.tests.map(normalizeTestResult) : [];
  if (resp.lang === "JSON" || resp.lang === "XML" || resp.lang === "HTML" || resp.lang === "Text") {
    responseLang.value = resp.lang;
  }
  errorMessage.value = typeof resp.error === "string" ? resp.error : "";
}

function normalizeTestResult(raw: any): TestResult {
  return {
    name: String(raw?.name ?? "(unnamed test)"),
    passed: !!raw?.passed,
    error: String(raw?.error ?? ""),
    durationMs: Number(raw?.durationMs ?? 0) || 0,
  };
}

interface PmSandbox {
  response: {
    status: number;
    code: number;
    elapsedMs: number;
    sizeBytes: number;
    body: string;
    text: () => string;
    json: () => unknown;
    headers: {
      all: () => ResponseHeader[];
      get: (name: string) => string | null;
      has: (name: string) => boolean;
    };
  };
  expect: (actual: unknown) => Expectation;
  test: (name: string, fn: () => void) => void;
}

function buildPmForResponse(
  response: HistoryResponse,
  onResult?: (r: TestResult) => void,
): PmSandbox {
  const headers = response.headers ?? [];
  const headerLookup: Record<string, string> = {};
  for (const h of headers) headerLookup[h.key.toLowerCase()] = h.value;

  let cachedJson: unknown;
  let jsonParsed = false;

  return {
    response: {
      status: response.status ?? 0,
      code: response.status ?? 0,
      elapsedMs: response.elapsedMs ?? 0,
      sizeBytes: response.sizeBytes ?? 0,
      body: response.body ?? "",
      text: () => response.body ?? "",
      json: () => {
        if (!jsonParsed) {
          try {
            cachedJson = JSON.parse(response.body ?? "");
          } catch (err) {
            throw new Error(`response body is not valid JSON: ${String(err)}`);
          }
          jsonParsed = true;
        }
        return cachedJson;
      },
      headers: {
        all: () => headers.slice(),
        get: (name: string) => headerLookup[String(name).toLowerCase()] ?? null,
        has: (name: string) => String(name).toLowerCase() in headerLookup,
      },
    },
    expect: (actual: unknown) => makeExpectation(actual),
    test: (name: string, fn: () => void) => {
      if (!onResult) {
        // Extraction sandbox: swallow pm.test calls silently.
        try {
          fn();
        } catch {
          /* ignore */
        }
        return;
      }
      const label = String(name ?? "(unnamed test)");
      const started = performance.now();
      try {
        fn();
        onResult({
          name: label,
          passed: true,
          error: "",
          durationMs: Math.round(performance.now() - started),
        });
      } catch (err) {
        onResult({
          name: label,
          passed: false,
          error: err instanceof Error ? err.message : String(err),
          durationMs: Math.round(performance.now() - started),
        });
      }
    },
  };
}

function runTestScript(
  script: string,
  response: HistoryResponse,
): TestResult[] {
  const trimmed = script.trim();
  if (!trimmed) return [];

  const results: TestResult[] = [];
  const pm = buildPmForResponse(response, (r) => results.push(r));

  try {
    const runner = new Function("pm", `"use strict";\n${trimmed}`);
    runner(pm);
  } catch (err) {
    results.push({
      name: "<script>",
      passed: false,
      error: err instanceof Error ? err.message : String(err),
      durationMs: 0,
    });
  }
  return results;
}

interface Expectation {
  to: {
    equal: (expected: unknown) => void;
    eql: (expected: unknown) => void;
    include: (expected: unknown) => void;
    have: { status: (code: number) => void };
    be: {
      oneOf: (options: unknown[]) => void;
      a: (type: string) => void;
      an: (type: string) => void;
      true: () => void;
      false: () => void;
      null: () => void;
      undefined: () => void;
    };
    match: (regex: RegExp) => void;
  };
  not: Expectation;
}

function makeExpectation(actual: unknown, negated = false): Expectation {
  const check = (cond: boolean, msg: string) => {
    const ok = negated ? !cond : cond;
    if (!ok) throw new Error(negated ? `expected not: ${msg}` : msg);
  };
  const preview = (val: unknown) => {
    try {
      return JSON.stringify(val);
    } catch {
      return String(val);
    }
  };
  const api: Expectation = {
    to: {
      equal(expected) {
        check(actual === expected, `expected ${preview(actual)} === ${preview(expected)}`);
      },
      eql(expected) {
        check(deepEqual(actual, expected), `expected ${preview(actual)} to deep-equal ${preview(expected)}`);
      },
      include(expected) {
        if (typeof actual === "string") {
          check(actual.includes(String(expected)), `expected "${actual}" to include "${expected}"`);
        } else if (Array.isArray(actual)) {
          check(actual.includes(expected as any), `expected array to include ${preview(expected)}`);
        } else if (actual && typeof actual === "object" && expected && typeof expected === "object") {
          for (const [k, v] of Object.entries(expected as Record<string, unknown>)) {
            check(deepEqual((actual as any)[k], v), `expected object to include ${k}=${preview(v)}`);
          }
        } else {
          check(false, `cannot .include on ${typeof actual}`);
        }
      },
      have: {
        status(code) {
          check(actual === code, `expected status ${actual} to be ${code}`);
        },
      },
      be: {
        oneOf(options) {
          check(options.includes(actual), `expected ${preview(actual)} to be one of ${preview(options)}`);
        },
        a(type) {
          check(typeof actual === type, `expected type ${typeof actual} to be ${type}`);
        },
        an(type) {
          check(typeof actual === type, `expected type ${typeof actual} to be ${type}`);
        },
        true() { check(actual === true, `expected ${preview(actual)} to be true`); },
        false() { check(actual === false, `expected ${preview(actual)} to be false`); },
        null() { check(actual === null, `expected ${preview(actual)} to be null`); },
        undefined() { check(actual === undefined, `expected ${preview(actual)} to be undefined`); },
      },
      match(regex) {
        check(regex.test(String(actual)), `expected ${preview(actual)} to match ${regex}`);
      },
    },
    get not() {
      return makeExpectation(actual, !negated);
    },
  };
  return api;
}

function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (a === null || b === null || typeof a !== "object" || typeof b !== "object") return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  const ka = Object.keys(a as any);
  const kb = Object.keys(b as any);
  if (ka.length !== kb.length) return false;
  for (const k of ka) if (!deepEqual((a as any)[k], (b as any)[k])) return false;
  return true;
}

function isRequestSending(requestId: string): boolean {
  return !!sendingRequestIds.value[requestId];
}

function setRequestSending(requestId: string, sending: boolean) {
  const next = { ...sendingRequestIds.value };
  if (sending) {
    next[requestId] = true;
  } else {
    delete next[requestId];
  }
  sendingRequestIds.value = next;
}

function clearRequestTimeout(requestId: string) {
  const timeout = sendingTimeouts[requestId];
  if (timeout) {
    clearTimeout(timeout);
    delete sendingTimeouts[requestId];
  }
}

function startRequestSending(requestId: string, onTimeout: () => void | Promise<void>): number {
  const runId = ++sendRunSeq;
  activeSendRunIds[requestId] = runId;
  clearRequestTimeout(requestId);
  setRequestSending(requestId, true);
  sendingTimeouts[requestId] = setTimeout(() => {
    if (activeSendRunIds[requestId] !== runId) return;
    delete activeSendRunIds[requestId];
    delete sendingTimeouts[requestId];
    setRequestSending(requestId, false);
    void onTimeout();
  }, REQUEST_TIMEOUT_MS);
  return runId;
}

function isCurrentSendRun(requestId: string, runId: number): boolean {
  return activeSendRunIds[requestId] === runId;
}

function finishRequestSending(requestId: string, runId: number) {
  if (!isCurrentSendRun(requestId, runId)) return;
  delete activeSendRunIds[requestId];
  clearRequestTimeout(requestId);
  setRequestSending(requestId, false);
}

function clearAllRequestTimeouts() {
  Object.keys(sendingTimeouts).forEach(clearRequestTimeout);
}

function applyResponseIfActive(requestId: string, resp: HistoryResponse | undefined) {
  if (activeTabId.value === requestId) {
    applyResponseSnap(resp);
  }
}

function rememberResponse(requestId: string, resp: HistoryResponse) {
  lastResponses.value = { ...lastResponses.value, [requestId]: resp };
}

async function recordTimeoutResponse(
  requestId: string,
  method: HttpMethod,
  url: string,
  snapshot: HistorySnapshot,
) {
  const responseSnap: HistoryResponse = {
    status: null,
    headers: [],
    body: "",
    elapsedMs: REQUEST_TIMEOUT_MS,
    sizeBytes: null,
    error: REQUEST_TIMEOUT_MESSAGE,
  };
  const req = requests.value.find((r) => r.id === requestId);
  if (req?.tests) responseSnap.tests = runTestScript(req.tests, responseSnap);
  rememberResponse(requestId, responseSnap);
  applyResponseIfActive(requestId, responseSnap);
  appendHistoryEntry({
    id: uuid(),
    method,
    url,
    status: null,
    timestamp: Date.now(),
    snapshot,
    response: responseSnap,
  });
  await recordHistoryRemote({
    request_id: requestId,
    method,
    url,
    status: null,
    elapsed_ms: REQUEST_TIMEOUT_MS,
    error: REQUEST_TIMEOUT_MESSAGE,
    snapshot,
    response: responseSnap,
  });
}

function forgetResponse(requestId: string) {
  delete activeSendRunIds[requestId];
  clearRequestTimeout(requestId);
  setRequestSending(requestId, false);
  if (requestId in lastResponses.value) {
    const next = { ...lastResponses.value };
    delete next[requestId];
    lastResponses.value = next;
  }
}

function openTab(requestId: string) {
  if (!openTabIds.value.includes(requestId)) {
    openTabIds.value.push(requestId);
  }
  activeTabId.value = requestId;
  const owner = requests.value.find((r) => r.id === requestId);
  if (owner) selectedFolderId.value = owner.folderId;
  // Response panel is restored by the `activeTabId` watcher below from
  // `lastResponses`, so we intentionally do not clear it here.
}

const TEST_SNIPPETS: Record<string, string> = {
  status: `pm.test("状态码为 200", () => {\n  pm.expect(pm.response.status).to.equal(200);\n});\n`,
  time: `pm.test("响应耗时 < 500ms", () => {\n  pm.expect(pm.response.elapsedMs).to.be.a("number");\n  if (pm.response.elapsedMs >= 500) throw new Error("too slow: " + pm.response.elapsedMs + "ms");\n});\n`,
  json: `pm.test("响应体是 JSON 且包含 code 字段", () => {\n  const data = pm.response.json();\n  pm.expect(data).to.be.an("object");\n  pm.expect(data.code).to.equal(0);\n});\n`,
  header: `pm.test("Content-Type 包含 application/json", () => {\n  const ct = pm.response.headers.get("content-type") || "";\n  pm.expect(ct).to.include("application/json");\n});\n`,
};

function insertTestSnippet(kind: string) {
  const req = activeRequest.value;
  if (!req) return;
  const snippet = TEST_SNIPPETS[kind];
  if (!snippet) return;
  const prev = req.tests ?? "";
  req.tests = prev ? `${prev.replace(/\s+$/, "")}\n\n${snippet}` : snippet;
}

function formatTestScript() {
  const req = activeRequest.value;
  if (!req?.tests) return;
  try {
    req.tests = formatJs(req.tests);
  } catch {
    // Fall back to no-op on unparseable input to avoid destroying user code.
  }
}

const testsEditorEl = ref<HTMLTextAreaElement | null>(null);
const testsHighlightEl = ref<HTMLPreElement | null>(null);

const JS_KEYWORDS = new Set([
  "var", "let", "const", "function", "return", "if", "else", "for", "while",
  "do", "switch", "case", "break", "continue", "default", "throw", "try",
  "catch", "finally", "new", "delete", "typeof", "instanceof", "in", "of",
  "void", "yield", "async", "await", "class", "extends", "super", "this",
  "import", "export", "from", "as",
]);
const JS_LITERALS = new Set(["true", "false", "null", "undefined", "NaN", "Infinity"]);
const JS_BUILTINS = new Set(["pm", "console", "Math", "JSON", "Date", "Object", "Array", "String", "Number", "Boolean", "RegExp", "Promise", "Error"]);

const highlightedTests = computed(() => {
  const source = activeRequest.value?.tests ?? "";
  if (!source) return "";
  const html = highlightJs(source);
  // Trailing newline keeps <pre> height in sync with the textarea's implicit
  // extra line when the buffer ends without one.
  return html + (source.endsWith("\n") ? "\n" : "");
});

function highlightJs(source: string): string {
  const tokens = tokenizeJs(source);
  let out = "";
  for (const tk of tokens) {
    if (tk.kind === "ws") {
      out += escapeHtml(tk.value);
      continue;
    }
    if (tk.kind === "comment") {
      out += `<span class="tk-c">${escapeHtml(tk.value)}</span>`;
      continue;
    }
    if (tk.kind === "string" || tk.kind === "template") {
      out += `<span class="tk-s">${escapeHtml(tk.value)}</span>`;
      continue;
    }
    if (tk.kind === "regex") {
      out += `<span class="tk-r">${escapeHtml(tk.value)}</span>`;
      continue;
    }
    // punct-kind tokens include identifiers, keywords, numbers, operators.
    const value = tk.value;
    if (/^[0-9]/.test(value) && /^[0-9.]+$/.test(value)) {
      out += `<span class="tk-n">${escapeHtml(value)}</span>`;
      continue;
    }
    if (/^[A-Za-z_$][\w$]*$/.test(value)) {
      if (JS_KEYWORDS.has(value)) out += `<span class="tk-k">${value}</span>`;
      else if (JS_LITERALS.has(value)) out += `<span class="tk-l">${value}</span>`;
      else if (JS_BUILTINS.has(value)) out += `<span class="tk-b">${value}</span>`;
      else out += `<span class="tk-i">${escapeHtml(value)}</span>`;
      continue;
    }
    // Dotted identifier chain (e.g. `pm.response.status`): split so we can style parts.
    if (/^[A-Za-z_$][\w$.]*$/.test(value) && value.includes(".")) {
      const parts = value.split(".");
      const rendered = parts
        .map((part) => {
          if (!part) return "";
          if (JS_BUILTINS.has(part)) return `<span class="tk-b">${part}</span>`;
          if (JS_KEYWORDS.has(part)) return `<span class="tk-k">${part}</span>`;
          if (JS_LITERALS.has(part)) return `<span class="tk-l">${part}</span>`;
          return `<span class="tk-p">${escapeHtml(part)}</span>`;
        })
        .join('<span class="tk-op">.</span>');
      out += rendered;
      continue;
    }
    out += `<span class="tk-op">${escapeHtml(value)}</span>`;
  }
  return out;
}

function syncTestsScroll() {
  const ta = testsEditorEl.value;
  const hl = testsHighlightEl.value;
  if (!ta || !hl) return;
  hl.scrollTop = ta.scrollTop;
  hl.scrollLeft = ta.scrollLeft;
}

watch(
  () => activeRequest.value?.tests,
  () => {
    nextTick(syncTestsScroll);
  },
);

/**
 * Minimal JS beautifier: re-indents based on `{}[]()` depth and inserts
 * line breaks after `{`, `;`, and before `}`. String / template literals,
 * comments and regex literals are treated as opaque tokens so their
 * contents survive untouched.
 */
function formatJs(source: string): string {
  const INDENT = "  ";
  const tokens = tokenizeJs(source);
  let depth = 0;
  let out = "";
  let atLineStart = true;
  const startLine = () => {
    out = out.replace(/[ \t]+$/, "");
    if (!out.endsWith("\n") && out.length > 0) out += "\n";
    out += INDENT.repeat(Math.max(0, depth));
    atLineStart = true;
  };
  const append = (text: string) => {
    if (!text) return;
    out += text;
    atLineStart = false;
  };

  for (let i = 0; i < tokens.length; i++) {
    const tk = tokens[i];
    if (tk.kind === "ws") {
      // Preserve intentional blank lines (>= 2 newlines) between statements.
      const newlineCount = (tk.value.match(/\n/g) ?? []).length;
      if (newlineCount >= 2 && !atLineStart) {
        out = out.replace(/[ \t]+$/, "");
        out += "\n";
        startLine();
      } else if (!atLineStart) {
        // Collapse whitespace to a single space unless we just emitted a line break.
        if (!out.endsWith(" ") && !out.endsWith("\n")) append(" ");
      }
      continue;
    }
    if (tk.kind === "comment") {
      if (tk.value.startsWith("//") && !atLineStart) append(" ");
      append(tk.value);
      if (tk.value.startsWith("//")) startLine();
      continue;
    }
    if (tk.kind === "string" || tk.kind === "regex" || tk.kind === "template") {
      append(tk.value);
      continue;
    }
    // Punctuation / identifiers.
    const value = tk.value;
    if (value === "{") {
      if (!atLineStart && !out.endsWith(" ")) append(" ");
      append("{");
      depth++;
      startLine();
    } else if (value === "}") {
      depth = Math.max(0, depth - 1);
      startLine();
      append("}");
      // If next non-ws token is not a closer/punctuator, break.
      const next = peekNextNonWs(tokens, i);
      if (
        next &&
        next.kind === "punct" &&
        (next.value === "else" || next.value === "catch" || next.value === "finally")
      ) {
        append(" ");
      } else if (
        !next ||
        (next.kind === "punct" &&
          [")", "]", "}", ",", ";", ".", "?", ":"].includes(next.value))
      ) {
        // stay inline
      } else {
        startLine();
      }
    } else if (value === ";") {
      append(";");
      const next = peekNextNonWs(tokens, i);
      if (next && next.kind === "punct" && next.value === ")") {
        // `for (;;)` shape — keep on same line
        append(" ");
      } else {
        startLine();
      }
    } else if (value === ",") {
      append(",");
      if (depth > 0) append(" ");
    } else {
      append(value);
    }
  }
  return out.replace(/[ \t]+$/gm, "").replace(/\n{3,}/g, "\n\n").trim() + "\n";
}

interface JsToken {
  kind: "ws" | "comment" | "string" | "template" | "regex" | "punct";
  value: string;
}

function peekNextNonWs(tokens: JsToken[], from: number): JsToken | null {
  for (let j = from + 1; j < tokens.length; j++) {
    if (tokens[j].kind !== "ws") return tokens[j];
  }
  return null;
}

function tokenizeJs(source: string): JsToken[] {
  const tokens: JsToken[] = [];
  let i = 0;
  const n = source.length;
  // Track the previous meaningful token so we can decide whether `/` starts a regex.
  const regexAfter = new Set([
    "=", "(", ",", ";", ":", "!", "&", "|", "?", "{", "}", "[",
    "return", "typeof", "instanceof", "in", "of", "new", "throw",
    "delete", "void", "case", "do", "else",
  ]);
  let lastMeaningful = ";"; // start-of-file behaves like statement boundary
  const emit = (kind: JsToken["kind"], value: string) => {
    tokens.push({ kind, value });
    if (kind !== "ws" && kind !== "comment") lastMeaningful = value;
  };
  while (i < n) {
    const ch = source[i];
    // Whitespace.
    if (/\s/.test(ch)) {
      let j = i;
      while (j < n && /\s/.test(source[j])) j++;
      tokens.push({ kind: "ws", value: source.slice(i, j) });
      i = j;
      continue;
    }
    // Line comment.
    if (ch === "/" && source[i + 1] === "/") {
      let j = i + 2;
      while (j < n && source[j] !== "\n") j++;
      emit("comment", source.slice(i, j));
      i = j;
      continue;
    }
    // Block comment.
    if (ch === "/" && source[i + 1] === "*") {
      let j = i + 2;
      while (j < n && !(source[j] === "*" && source[j + 1] === "/")) j++;
      j = Math.min(n, j + 2);
      emit("comment", source.slice(i, j));
      i = j;
      continue;
    }
    // Regex literal.
    if (ch === "/" && regexAfter.has(lastMeaningful)) {
      let j = i + 1;
      let inClass = false;
      while (j < n) {
        const c = source[j];
        if (c === "\\") { j += 2; continue; }
        if (c === "[") inClass = true;
        else if (c === "]") inClass = false;
        else if (c === "/" && !inClass) { j++; break; }
        else if (c === "\n") break;
        j++;
      }
      while (j < n && /[a-z]/i.test(source[j])) j++;
      emit("regex", source.slice(i, j));
      i = j;
      continue;
    }
    // String literals.
    if (ch === "'" || ch === '"') {
      const quote = ch;
      let j = i + 1;
      while (j < n) {
        if (source[j] === "\\") { j += 2; continue; }
        if (source[j] === quote) { j++; break; }
        if (source[j] === "\n") break;
        j++;
      }
      emit("string", source.slice(i, j));
      i = j;
      continue;
    }
    // Template literal (no ${} nesting depth handling — good enough for test scripts).
    if (ch === "`") {
      let j = i + 1;
      while (j < n) {
        if (source[j] === "\\") { j += 2; continue; }
        if (source[j] === "`") { j++; break; }
        j++;
      }
      emit("template", source.slice(i, j));
      i = j;
      continue;
    }
    // Multi-char operators / punctuation we care about grouping.
    const three = source.substr(i, 3);
    if (["===", "!==", "...", ">>>"].includes(three)) {
      emit("punct", three);
      i += 3;
      continue;
    }
    const two = source.substr(i, 2);
    if (
      ["==", "!=", "<=", ">=", "&&", "||", "=>", "++", "--", "+=", "-=", "*=", "/=", "??"].includes(two)
    ) {
      emit("punct", two);
      i += 2;
      continue;
    }
    // Identifier / keyword / number.
    if (/[A-Za-z_$0-9]/.test(ch)) {
      let j = i;
      while (j < n && /[A-Za-z_$0-9.]/.test(source[j])) j++;
      emit("punct", source.slice(i, j));
      i = j;
      continue;
    }
    // Single char punctuation.
    emit("punct", ch);
    i++;
  }
  return tokens;
}

watch(activeTabId, (id) => {
  applyResponseSnap(id ? lastResponses.value[id] : undefined);
});

function closeTab(requestId: string) {
  const idx = openTabIds.value.indexOf(requestId);
  if (idx === -1) return;
  openTabIds.value.splice(idx, 1);
  if (activeTabId.value === requestId) {
    activeTabId.value =
      openTabIds.value[Math.max(0, idx - 1)] ?? openTabIds.value[0] ?? "";
  }
}

function newTab(kind: RequestKind = "http") {
  const folder = resolveTargetFolder();
  addRequestToFolder(folder, kind);
  closeDropdown();
}

function openHistoryEntry(entry: HistoryEntry) {
  const folder = resolveTargetFolder();
  folder.expanded = true;
  selectedFolderId.value = folder.id;
  const snap = entry.snapshot && typeof entry.snapshot === "object" ? entry.snapshot : undefined;
  const hasSnap = !!snap && Object.keys(snap).length > 0;
  const kind: RequestKind = snap?.kind === "grpc" ? "grpc" : "http";
  const fallbackName =
    kind === "grpc" && snap?.grpc?.method?.trim()
      ? `${snap.grpc.service ?? ""}/${snap.grpc.method}`
      : deriveRequestNameFromUrl(entry.url);
  const name = snap?.name?.trim() || fallbackName;
  const record = makeRequest(folder.id, name, kind);
  record.method = entry.method ?? record.method;
  record.url = entry.url ?? record.url;
  if (hasSnap && snap) {
    if (typeof snap.method === "string" && snap.method.trim()) {
      record.method = snap.method as HttpMethod;
    }
    if (typeof snap.url === "string") record.url = snap.url;
    const params = cloneKvList(snap.params);
    if (params.length) record.params = params;
    const headers = cloneKvList(snap.headers);
    if (headers.length) record.headers = headers;
    if (typeof snap.bodyType === "string") record.bodyType = snap.bodyType as BodyType;
    if (typeof snap.rawLang === "string") record.rawLang = snap.rawLang as RawLang;
    if (typeof snap.body === "string") record.body = snap.body;
    record.bodyRows = normalizeBodyRows(snap.bodyRows);
    if (typeof snap.binaryPath === "string") record.binaryPath = snap.binaryPath;
    if (snap.grpc && typeof snap.grpc === "object") {
      const metadata = cloneKvList(snap.grpc.metadata);
      record.grpc = {
        ...record.grpc,
        ...snap.grpc,
        metadata: metadata.length ? metadata : record.grpc.metadata,
      };
    }
  }
  requests.value.push(record);
  // Pre-seed the per-tab response cache before opening the tab so the
  // `activeTabId` watcher restores the stored response automatically.
  if (entry.response && typeof entry.response === "object") {
    rememberResponse(record.id, entry.response);
  }
  openTab(record.id);
}

function deriveRequestNameFromUrl(url: string): string {
  const trimmed = (url ?? "").trim();
  if (!trimmed) return "新建请求";
  try {
    const u = new URL(/^[a-z][a-z0-9+.\-]*:\/\//i.test(trimmed) ? trimmed : `http://${trimmed}`);
    const path = u.pathname && u.pathname !== "/" ? u.pathname : "";
    return `${u.host}${path}` || trimmed;
  } catch {
    return trimmed.length > 64 ? `${trimmed.slice(0, 64)}…` : trimmed;
  }
}

function onClickEnv(env: EnvRecord) {
  selectedEnvId.value = env.id;
  if (env.kind !== "global") {
    activeEnvId.value = env.id;
  }
}

async function addEnvironment() {
  const name = await openPrompt({
    title: "新建环境",
    label: "环境名称",
    placeholder: "例如：测试环境",
    confirmText: "创建",
  });
  if (!name) return;
  const env: EnvRecord = {
    id: uuid(),
    name,
    kind: "custom",
    variables: [emptyRow()],
    headers: [emptyRow()],
  };
  environments.value.push(env);
  selectedEnvId.value = env.id;
}

async function deleteEnvironment(env: EnvRecord) {
  if (env.kind !== "custom") return;
  const ok = await openConfirm({
    title: "删除环境",
    message: `确定删除环境「${env.name}」吗？`,
    confirmText: "删除",
    danger: true,
  });
  if (!ok) return;
  environments.value = environments.value.filter((e) => e.id !== env.id);
  if (selectedEnvId.value === env.id) {
    selectedEnvId.value = globalEnv.value?.id ?? "";
  }
  if (activeEnvId.value === env.id) {
    const fallback = environments.value.find((e) => e.kind !== "global");
    activeEnvId.value = fallback?.id ?? "";
  }
}

function onRenameRequest(event: Event) {
  const target = event.target as HTMLElement;
  const newName = (target.textContent ?? "").trim();
  if (!activeRequest.value) return;
  if (!newName) {
    target.textContent = activeRequest.value.name;
    return;
  }
  activeRequest.value.name = newName;
}

function onRenameRequestEnter(event: KeyboardEvent) {
  (event.target as HTMLElement).blur();
}

function normalizeUrlScheme(url: string): string {
  const trimmed = url.trim();
  if (!trimmed) return trimmed;
  if (/^[a-z][a-z0-9+.\-]*:\/\//i.test(trimmed)) return trimmed;
  if (trimmed.startsWith("//")) return `http:${trimmed}`;
  if (trimmed.startsWith("{{")) return trimmed;
  return `http://${trimmed}`;
}

function bodyRowsFromUrlEncoded(body: string): BodyFormRow[] {
  const rows: BodyFormRow[] = [];
  const text = body.trim();
  if (text) {
    new URLSearchParams(text).forEach((value, key) => {
      rows.push({
        ...emptyBodyRow(),
        key,
        value,
      });
    });
  }
  rows.push(emptyBodyRow());
  return rows;
}

function tokenizeCurl(input: string): string[] {
  const cleaned = input
    .replace(/\\\r?\n/g, " ")
    .replace(/[\r\n]+/g, " ")
    .trim();
  const tokens: string[] = [];
  let i = 0;
  while (i < cleaned.length) {
    const c = cleaned[i];
    if (c === " " || c === "\t") {
      i++;
      continue;
    }
    if (c === '"' || c === "'") {
      const quote = c;
      let j = i + 1;
      let buf = "";
      while (j < cleaned.length) {
        const cc = cleaned[j];
        if (cc === "\\" && quote === '"' && j + 1 < cleaned.length) {
          buf += cleaned[j + 1];
          j += 2;
          continue;
        }
        if (cc === quote) {
          j++;
          break;
        }
        buf += cc;
        j++;
      }
      tokens.push(buf);
      i = j;
    } else {
      let j = i;
      let buf = "";
      while (j < cleaned.length && cleaned[j] !== " " && cleaned[j] !== "\t") {
        if (cleaned[j] === "\\" && j + 1 < cleaned.length) {
          buf += cleaned[j + 1];
          j += 2;
          continue;
        }
        buf += cleaned[j];
        j++;
      }
      tokens.push(buf);
      i = j;
    }
  }
  return tokens;
}

function parseCurl(input: string): ParsedCurl | null {
  if (!input.trim()) return null;
  const tokens = tokenizeCurl(input);
  if (!tokens.length) return null;
  if (tokens[0].toLowerCase() === "curl") tokens.shift();
  if (!tokens.length) return null;

  let method: HttpMethod = "GET";
  let url = "";
  const headers: { key: string; value: string }[] = [];
  let body = "";
  let methodSetExplicitly = false;
  const valueFlags = new Set([
    "--referer", "-e", "--user-agent", "-A",
    "--connect-timeout", "--max-time", "-m", "-o", "--output",
    "--proxy", "-x", "--cacert", "--cert", "--key", "--resolve",
    "--form-string", "-F", "--form",
  ]);

  for (let i = 0; i < tokens.length; i++) {
    const t = tokens[i];
    const next = () => tokens[++i] ?? "";
    if (t === "-X" || t === "--request") {
      const m = next().toUpperCase();
      if (["GET", "POST", "PUT", "PATCH", "DELETE"].includes(m)) {
        method = m as HttpMethod;
      }
      methodSetExplicitly = true;
    } else if (t === "-H" || t === "--header") {
      const v = next();
      const idx = v.indexOf(":");
      if (idx > 0) {
        headers.push({
          key: v.slice(0, idx).trim(),
          value: v.slice(idx + 1).trim(),
        });
      }
    } else if (
      t === "-d" || t === "--data" || t === "--data-raw" ||
      t === "--data-binary" || t === "--data-ascii" || t === "--data-urlencode"
    ) {
      const v = next();
      body += body ? `&${v}` : v;
      if (!methodSetExplicitly) method = "POST";
    } else if (t === "-u" || t === "--user") {
      const cred = next();
      try {
        headers.push({ key: "Authorization", value: `Basic ${btoa(cred)}` });
      } catch {
        headers.push({ key: "Authorization", value: `Basic ${cred}` });
      }
    } else if (t === "-b" || t === "--cookie") {
      const v = next();
      // curl treats values containing `=` as inline cookies; otherwise it's a cookie jar file path
      if (v && v.includes("=")) {
        const existing = headers.find((h) => h.key.toLowerCase() === "cookie");
        if (existing) {
          existing.value = existing.value ? `${existing.value}; ${v}` : v;
        } else {
          headers.push({ key: "Cookie", value: v });
        }
      }
    } else if (t === "--url") {
      url = next();
    } else if (t === "-G" || t === "--get") {
      method = "GET";
      methodSetExplicitly = true;
    } else if (t.startsWith("-")) {
      if (valueFlags.has(t)) i++;
    } else if (!url) {
      url = t;
    }
  }

  if (!url) return null;

  let bodyType: BodyType = body ? "raw" : "none";
  let rawLang: RawLang = "Text";
  const ct =
    headers.find((h) => h.key.toLowerCase() === "content-type")?.value ?? "";
  if (body) {
    if (ct.includes("application/x-www-form-urlencoded")) {
      bodyType = "x-www-form-urlencoded";
    } else if (ct.includes("json")) {
      rawLang = "JSON";
    } else if (ct.includes("xml")) {
      rawLang = "XML";
    } else if (ct.includes("html")) {
      rawLang = "HTML";
    } else if (ct.includes("javascript")) {
      rawLang = "JavaScript";
    } else if (/^\s*[\[{]/.test(body)) {
      rawLang = "JSON";
    }
  }

  if (rawLang === "JSON") {
    const formatted = tryFormatJson(body);
    if (formatted !== null) body = formatted;
  }

  return { method, url, headers, body, bodyType, rawLang };
}

function openImportModal() {
  importCurlText.value = "";
  importParsed.value = null;
  importError.value = "";
  importNewFolderName.value = "";
  importRequestName.value = "";
  importSource.value = "curl";
  importTargetFolderId.value = folders.value[0]?.id ?? "__new__";
  showImportModal.value = true;
}

function deriveRequestName(p: ParsedCurl): string {
  try {
    const u = new URL(normalizeUrlScheme(p.url));
    const path = u.pathname && u.pathname !== "/" ? u.pathname : "";
    return `${p.method} ${path || u.host}`.trim();
  } catch {
    return `${p.method} ${p.url}`.trim();
  }
}

watch(importParsed, (p) => {
  if (p && !importRequestName.value.trim()) {
    importRequestName.value = deriveRequestName(p);
  }
});

function closeImportModal() {
  showImportModal.value = false;
}

function tryParseCurl() {
  importError.value = "";
  const text = importCurlText.value.trim();
  if (!text) {
    importParsed.value = null;
    importError.value = "请粘贴 cURL 命令";
    return;
  }
  if (!/^\s*curl\b/i.test(text)) {
    importParsed.value = null;
    importError.value = "不是合法的 cURL 命令（需以 curl 开头）";
    return;
  }
  const parsed = parseCurl(text);
  if (!parsed || !parsed.url) {
    importParsed.value = null;
    importError.value = "解析失败，请检查命令格式";
    return;
  }
  importParsed.value = parsed;
}

watch(importCurlText, () => {
  if (!importCurlText.value.trim()) {
    importParsed.value = null;
    importError.value = "";
    return;
  }
  tryParseCurl();
});

function confirmImport() {
  if (!importParsed.value) {
    tryParseCurl();
    if (!importParsed.value) return;
  }
  let folderId = importTargetFolderId.value;
  if (folderId === "__new__") {
    const name = importNewFolderName.value.trim() || "导入";
    const folder = makeFolder(name);
    folders.value.push(folder);
    folderId = folder.id;
  }
  const p = importParsed.value;
  const displayName = importRequestName.value.trim() || deriveRequestName(p);
  const record: RequestRecord = {
    id: uuid(),
    folderId,
    name: displayName,
    kind: "http",
    method: p.method,
    url: p.url,
    params: [emptyRow()],
    grpc: makeGrpcConfig(),
    headers: [
      ...p.headers.map((h) => ({
        enabled: true,
        key: h.key,
        value: h.value,
        description: "",
      })),
      emptyRow(),
    ],
    bodyType: p.bodyType,
    rawLang: p.rawLang,
    body: p.body,
    bodyRows: p.bodyType === "x-www-form-urlencoded"
      ? bodyRowsFromUrlEncoded(p.body)
      : [emptyBodyRow()],
    binaryPath: "",
    tests: "",
  };
  requests.value.push(record);
  const folder = folders.value.find((f) => f.id === folderId);
  if (folder) folder.expanded = true;
  openTab(record.id);
  closeImportModal();
}

let dragMode: "" | "sidebar" | "rows" = "";
let dragStartPos = 0;
let dragStartSize = 0;

function startSidebarDrag(event: MouseEvent) {
  dragMode = "sidebar";
  dragStartPos = event.clientX;
  dragStartSize = sidebarWidth.value;
  document.body.classList.add("dragging-x");
  window.addEventListener("mousemove", onDragMove);
  window.addEventListener("mouseup", onDragEnd);
  event.preventDefault();
}

function startRowsDrag(event: MouseEvent) {
  dragMode = "rows";
  dragStartPos = event.clientY;
  dragStartSize = requestPaneHeight.value;
  document.body.classList.add("dragging-y");
  window.addEventListener("mousemove", onDragMove);
  window.addEventListener("mouseup", onDragEnd);
  event.preventDefault();
}

function onDragMove(event: MouseEvent) {
  if (dragMode === "sidebar") {
    const delta = event.clientX - dragStartPos;
    sidebarWidth.value = clamp(dragStartSize + delta, SIDEBAR_MIN, SIDEBAR_MAX);
  } else if (dragMode === "rows") {
    const delta = event.clientY - dragStartPos;
    requestPaneHeight.value = clamp(
      dragStartSize + delta,
      REQUEST_PANE_MIN,
      REQUEST_PANE_MAX,
    );
  }
}

function onDragEnd() {
  dragMode = "";
  document.body.classList.remove("dragging-x");
  document.body.classList.remove("dragging-y");
  window.removeEventListener("mousemove", onDragMove);
  window.removeEventListener("mouseup", onDragEnd);
}

function isOnScrollbar(event: MouseEvent, target: HTMLElement | null): boolean {
  if (!target) return false;
  const hasVScroll = target.scrollHeight > target.clientHeight;
  const hasHScroll = target.scrollWidth > target.clientWidth;
  if (hasVScroll && event.offsetX > target.clientWidth) return true;
  if (hasHScroll && event.offsetY > target.clientHeight) return true;
  return false;
}

function startWindowDrag(event: MouseEvent) {
  if (event.button !== 0) return;
  const target = event.target as HTMLElement | null;
  if (isOnScrollbar(event, target)) return;
  if (
    target?.closest(
      [
        "button",
        "input",
        "textarea",
        "select",
        "a",
        "[role='button']",
        ".no-window-drag",
        ".search",
        ".panel-header",
        ".tree-row",
        ".tree-node",
        ".env-item",
        ".env-editor",
        ".history-item",
        ".kv-table",
        ".table-wrap",
      ].join(", "),
    )
  ) {
    return;
  }
  void getCurrentWindow().startDragging();
}

function buildEffectiveUrl(req: RequestRecord): string {
  const base = normalizeUrlScheme(substituteVars(req.url.trim()));
  const params = req.params.filter((p) => p.enabled && p.key.trim());
  if (!params.length) return base;
  try {
    const url = new URL(base);
    params.forEach((p) =>
      url.searchParams.append(p.key.trim(), substituteVars(p.value)),
    );
    return url.toString();
  } catch {
    const qs = params
      .map(
        (p) =>
          `${encodeURIComponent(p.key.trim())}=${encodeURIComponent(
            substituteVars(p.value),
          )}`,
      )
      .join("&");
    if (!qs) return base;
    return `${base}${base.includes("?") ? "&" : "?"}${qs}`;
  }
}

function buildEffectiveHeaders(req: RequestRecord): ResponseHeader[] {
  const requestHeaderKeys = new Set(
    req.headers
      .filter((h) => h.enabled && h.key.trim())
      .map((h) => h.key.trim().toLowerCase()),
  );
  const activeHeaderKeys = new Set(
    activeEnv.value?.id && activeEnv.value.id !== globalEnv.value?.id
      ? activeEnv.value.headers
          .filter((h) => h.enabled && h.key.trim())
          .map((h) => h.key.trim().toLowerCase())
      : [],
  );
  const envHeaders = [
    ...(globalEnv.value?.headers ?? []).filter(
      (h) => !activeHeaderKeys.has(h.key.trim().toLowerCase()),
    ),
    ...(activeEnv.value?.id && activeEnv.value.id !== globalEnv.value?.id
      ? activeEnv.value.headers
      : []),
  ];
  const headers = envHeaders
    .filter(
      (h) =>
        h.enabled &&
        h.key.trim() &&
        !requestHeaderKeys.has(h.key.trim().toLowerCase()),
    )
    .map((h) => ({ key: h.key.trim(), value: substituteVars(h.value) }));

  headers.push(
    ...req.headers
    .filter((h) => h.enabled && h.key.trim())
      .map((h) => ({ key: h.key.trim(), value: substituteVars(h.value) })),
  );

  const hasContentType = (h: ResponseHeader) =>
    h.key.toLowerCase() === "content-type";

  if (req.bodyType === "raw" && req.body.trim()) {
    if (!headers.some(hasContentType)) {
      const map: Record<RawLang, string> = {
        Text: "text/plain",
        JSON: "application/json",
        XML: "application/xml",
        HTML: "text/html",
        JavaScript: "application/javascript",
      };
      headers.push({ key: "Content-Type", value: map[req.rawLang] });
    }
  } else if (req.bodyType === "x-www-form-urlencoded") {
    if (!headers.some(hasContentType)) {
      headers.push({
        key: "Content-Type",
        value: "application/x-www-form-urlencoded",
      });
    }
  } else if (req.bodyType === "form-data") {
    return headers.filter((h) => !hasContentType(h));
  } else if (req.bodyType === "binary" && req.binaryPath.trim()) {
    if (!headers.some(hasContentType)) {
      headers.push({ key: "Content-Type", value: "application/octet-stream" });
    }
  }
  return headers;
}

interface HttpFormFieldPayload {
  enabled: boolean;
  key: string;
  value: string;
  field_type: BodyFormFieldType;
  file_path?: string;
  file_name?: string;
}

function buildEffectiveFormData(req: RequestRecord): HttpFormFieldPayload[] {
  return req.bodyRows
    .filter((row) => row.enabled && row.key.trim())
    .map((row) => ({
      enabled: true,
      key: row.key.trim(),
      value: substituteVars(row.value),
      field_type: row.fieldType,
      file_path: row.fieldType === "file" ? row.filePath : undefined,
      file_name: row.fieldType === "file" ? row.fileName : undefined,
    }));
}

function buildEffectiveBody(req: RequestRecord): string | undefined {
  if (req.bodyType === "none") return undefined;
  if (req.bodyType === "x-www-form-urlencoded") {
    const search = new URLSearchParams();
    req.bodyRows
      .filter((p) => p.enabled && p.key.trim())
      .forEach((p) => search.append(p.key.trim(), substituteVars(p.value)));
    const out = search.toString();
    return out || undefined;
  }
  if (req.bodyType === "raw") {
    const value = substituteVars(req.body);
    return value.length ? value : undefined;
  }
  return undefined;
}

interface GrpcMetadataPair {
  key: string;
  value: string;
}

interface GrpcResponsePayload {
  status: number;
  status_text: string;
  body: string;
  headers: ResponseHeader[];
  elapsed_ms: number;
  size_bytes: number;
}

async function sendRequest() {
  const req = activeRequest.value;
  if (!req) return;
  if (isRequestSending(req.id)) return;
  if (req.kind === "grpc") {
    await sendGrpcRequest(req);
    return;
  }
  if (!req.url.trim()) {
    errorMessage.value = "请先输入请求 URL";
    return;
  }
  const url = buildEffectiveUrl(req);
  const snapshot = snapshotFromRequest(req);
  const runId = startRequestSending(req.id, () =>
    recordTimeoutResponse(req.id, req.method, url, snapshot),
  );
  applyResponseIfActive(req.id, undefined);

  try {
    const headers = buildEffectiveHeaders(req);
    const body = buildEffectiveBody(req);

    const requestPayload = {
      method: req.method,
      url,
      headers,
      body_type: req.bodyType,
      body,
      form_data: req.bodyType === "form-data" ? buildEffectiveFormData(req) : [],
      binary_path: req.bodyType === "binary" ? req.binaryPath : undefined,
    };

    const probe = await invoke<HttpResponsePayload>("send_http_request", { payload: requestPayload });
    if (!isCurrentSendRun(req.id, runId)) return;

    const ct = probe.headers.find((h) => h.key.toLowerCase() === "content-type")?.value ?? "";
    const lang: ResponseLang = ct.includes("json") ? "JSON" : ct.includes("xml") ? "XML" : ct.includes("html") ? "HTML" : "Text";
    const responseSnap: HistoryResponse = {
      status: probe.status,
      headers: probe.headers.map((h) => ({ ...h })),
      body: probe.body,
      elapsedMs: probe.elapsed_ms,
      sizeBytes: probe.size_bytes,
      lang,
    };
    responseSnap.tests = runTestScript(req.tests, responseSnap);
    rememberResponse(req.id, responseSnap);
    applyResponseIfActive(req.id, responseSnap);
    appendHistoryEntry({ id: uuid(), method: req.method, url, status: probe.status, timestamp: Date.now(), snapshot, response: responseSnap });
    await recordHistoryRemote({ request_id: req.id, method: req.method, url, status: probe.status, elapsed_ms: probe.elapsed_ms, size_bytes: probe.size_bytes, snapshot, response: responseSnap });
  } catch (err) {
    if (!isCurrentSendRun(req.id, runId)) return;
    const message = String(err);
    const responseSnap: HistoryResponse = {
      status: null,
      headers: [],
      body: "",
      elapsedMs: null,
      sizeBytes: null,
      error: message,
    };
    responseSnap.tests = runTestScript(req.tests, responseSnap);
    rememberResponse(req.id, responseSnap);
    applyResponseIfActive(req.id, responseSnap);
    appendHistoryEntry({
      id: uuid(),
      method: req.method,
      url,
      status: null,
      timestamp: Date.now(),
      snapshot,
      response: responseSnap,
    });
    await recordHistoryRemote({
      request_id: req.id,
      method: req.method,
      url,
      status: null,
      error: message,
      snapshot,
      response: responseSnap,
    });
  } finally {
    finishRequestSending(req.id, runId);
  }
}

async function sendGrpcRequest(req: RequestRecord) {
  const g = req.grpc;
  const target = substituteVars(g.target).trim();
  if (!target) {
    errorMessage.value = "请先输入 gRPC target（host:port）";
    return;
  }
  if (!g.protoPath.trim()) {
    errorMessage.value = "请先在「Proto」页指定 .proto 文件路径";
    return;
  }
  if (!g.service.trim() || !g.method.trim()) {
    errorMessage.value = "请填写 Service 与 Method 名称";
    return;
  }
  const metadata: GrpcMetadataPair[] = g.metadata
    .filter((m) => m.enabled && m.key.trim())
    .map((m) => ({
      key: substituteVars(m.key).trim(),
      value: substituteVars(m.value),
    }));

  const importPaths = g.importPaths
    .split(/\r?\n/)
    .map((line) => substituteVars(line).trim())
    .filter(Boolean);

  const message = substituteVars(g.message).trim() || "{}";

  const summaryUrl = `grpc${g.useTls ? "s" : ""}://${target}/${g.service.trim()}/${g.method.trim()}`;
  const snapshot = snapshotFromRequest(req);
  const runId = startRequestSending(req.id, () =>
    recordTimeoutResponse(req.id, "POST", summaryUrl, snapshot),
  );
  applyResponseIfActive(req.id, undefined);

  try {
    const res = await invoke<GrpcResponsePayload>("send_grpc_request", {
      payload: {
        target,
        proto_path: substituteVars(g.protoPath).trim(),
        import_paths: importPaths,
        service: substituteVars(g.service).trim(),
        method: substituteVars(g.method).trim(),
        message,
        metadata,
        use_tls: g.useTls,
        authority: substituteVars(g.authority).trim() || null,
      },
    });
    if (!isCurrentSendRun(req.id, runId)) return;

    const histStatus = res.status === 0 ? 200 : 500;
    const histErr = res.status === 0 ? undefined : `${res.status} ${res.status_text}`;
    const responseSnap: HistoryResponse = {
      status: histStatus,
      headers: res.headers.map((h) => ({ ...h })),
      body: res.body,
      elapsedMs: res.elapsed_ms,
      sizeBytes: res.size_bytes,
      lang: "JSON",
      error: histErr,
    };
    responseSnap.tests = runTestScript(req.tests, responseSnap);
    rememberResponse(req.id, responseSnap);
    applyResponseIfActive(req.id, responseSnap);
    appendHistoryEntry({
      id: uuid(),
      method: "POST",
      url: summaryUrl,
      status: histStatus,
      timestamp: Date.now(),
      snapshot,
      response: responseSnap,
    });
    await recordHistoryRemote({
      request_id: req.id,
      method: "POST",
      url: summaryUrl,
      status: histStatus,
      elapsed_ms: res.elapsed_ms,
      size_bytes: res.size_bytes,
      error: histErr ?? null,
      snapshot,
      response: responseSnap,
    });
  } catch (err) {
    if (!isCurrentSendRun(req.id, runId)) return;
    const msg = String(err);
    const responseSnap: HistoryResponse = {
      status: null,
      headers: [],
      body: "",
      elapsedMs: null,
      sizeBytes: null,
      error: msg,
    };
    responseSnap.tests = runTestScript(req.tests, responseSnap);
    rememberResponse(req.id, responseSnap);
    applyResponseIfActive(req.id, responseSnap);
    appendHistoryEntry({
      id: uuid(),
      method: "POST",
      url: summaryUrl,
      status: null,
      timestamp: Date.now(),
      snapshot,
      response: responseSnap,
    });
    await recordHistoryRemote({
      request_id: req.id,
      method: "POST",
      url: summaryUrl,
      status: null,
      error: msg,
      snapshot,
      response: responseSnap,
    });
  } finally {
    finishRequestSending(req.id, runId);
  }
}

function tryFormatJson(text: string): string | null {
  const trimmed = text.trim();
  if (!trimmed || !/^[[{]/.test(trimmed)) return null;
  try {
    return JSON.stringify(JSON.parse(trimmed), null, 2);
  } catch {
    return null;
  }
}

// ---------- Flow execution engine ----------

function buildFlowUrl(req: RequestRecord, extra: Record<string, string>): string {
  const base = normalizeUrlScheme(substituteVars(req.url.trim(), extra));
  const params = req.params.filter((p) => p.enabled && p.key.trim());
  if (!params.length) return base;
  try {
    const url = new URL(base);
    params.forEach((p) =>
      url.searchParams.append(p.key.trim(), substituteVars(p.value, extra)),
    );
    return url.toString();
  } catch {
    const qs = params
      .map(
        (p) =>
          `${encodeURIComponent(p.key.trim())}=${encodeURIComponent(
            substituteVars(p.value, extra),
          )}`,
      )
      .join("&");
    if (!qs) return base;
    return `${base}${base.includes("?") ? "&" : "?"}${qs}`;
  }
}

function buildFlowHeaders(
  req: RequestRecord,
  extra: Record<string, string>,
): ResponseHeader[] {
  const requestHeaderKeys = new Set(
    req.headers
      .filter((h) => h.enabled && h.key.trim())
      .map((h) => h.key.trim().toLowerCase()),
  );
  const activeHeaderKeys = new Set(
    activeEnv.value?.id && activeEnv.value.id !== globalEnv.value?.id
      ? activeEnv.value.headers
          .filter((h) => h.enabled && h.key.trim())
          .map((h) => h.key.trim().toLowerCase())
      : [],
  );
  const envHeaders = [
    ...(globalEnv.value?.headers ?? []).filter(
      (h) => !activeHeaderKeys.has(h.key.trim().toLowerCase()),
    ),
    ...(activeEnv.value?.id && activeEnv.value.id !== globalEnv.value?.id
      ? activeEnv.value.headers
      : []),
  ];
  const headers: ResponseHeader[] = envHeaders
    .filter(
      (h) =>
        h.enabled &&
        h.key.trim() &&
        !requestHeaderKeys.has(h.key.trim().toLowerCase()),
    )
    .map((h) => ({ key: h.key.trim(), value: substituteVars(h.value, extra) }));
  headers.push(
    ...req.headers
      .filter((h) => h.enabled && h.key.trim())
      .map((h) => ({ key: h.key.trim(), value: substituteVars(h.value, extra) })),
  );
  const hasContentType = (h: ResponseHeader) =>
    h.key.toLowerCase() === "content-type";
  if (req.bodyType === "raw" && req.body.trim() && !headers.some(hasContentType)) {
    const map: Record<RawLang, string> = {
      Text: "text/plain",
      JSON: "application/json",
      XML: "application/xml",
      HTML: "text/html",
      JavaScript: "application/javascript",
    };
    headers.push({ key: "Content-Type", value: map[req.rawLang] });
  } else if (req.bodyType === "x-www-form-urlencoded" && !headers.some(hasContentType)) {
    headers.push({
      key: "Content-Type",
      value: "application/x-www-form-urlencoded",
    });
  } else if (req.bodyType === "form-data") {
    return headers.filter((h) => !hasContentType(h));
  } else if (
    req.bodyType === "binary" &&
    req.binaryPath.trim() &&
    !headers.some(hasContentType)
  ) {
    headers.push({ key: "Content-Type", value: "application/octet-stream" });
  }
  return headers;
}

function buildFlowBody(
  req: RequestRecord,
  extra: Record<string, string>,
): string | undefined {
  if (req.bodyType === "none") return undefined;
  if (req.bodyType === "x-www-form-urlencoded") {
    const search = new URLSearchParams();
    req.bodyRows
      .filter((p) => p.enabled && p.key.trim())
      .forEach((p) => search.append(p.key.trim(), substituteVars(p.value, extra)));
    const out = search.toString();
    return out || undefined;
  }
  if (req.bodyType === "raw") {
    const value = substituteVars(req.body, extra);
    return value.length ? value : undefined;
  }
  return undefined;
}

function buildFlowFormData(
  req: RequestRecord,
  extra: Record<string, string>,
): HttpFormFieldPayload[] {
  return req.bodyRows
    .filter((row) => row.enabled && row.key.trim())
    .map((row) => ({
      enabled: true,
      key: row.key.trim(),
      value: substituteVars(row.value, extra),
      field_type: row.fieldType,
      file_path: row.fieldType === "file" ? row.filePath : undefined,
      file_name: row.fieldType === "file" ? row.fileName : undefined,
    }));
}

/**
 * Execute a request without touching UI state. Returns a fully populated
 * HistoryResponse. Errors during transport are surfaced as a rejected promise
 * so callers can decide how to record them.
 */
async function executeRequestHeadless(
  req: RequestRecord,
  extra: Record<string, string>,
): Promise<{ url: string; response: HistoryResponse }> {
  if (req.kind === "grpc") {
    const g = req.grpc;
    const target = substituteVars(g.target, extra).trim();
    if (!target) throw new Error("gRPC target 未配置");
    const metadata = g.metadata
      .filter((m) => m.enabled && m.key.trim())
      .map((m) => ({
        key: substituteVars(m.key, extra).trim(),
        value: substituteVars(m.value, extra),
      }));
    const importPaths = g.importPaths
      .split(/\r?\n/)
      .map((line) => substituteVars(line, extra).trim())
      .filter(Boolean);
    const summaryUrl = `grpc${g.useTls ? "s" : ""}://${target}/${g.service.trim()}/${g.method.trim()}`;
    const res = await invoke<GrpcResponsePayload>("send_grpc_request", {
      payload: {
        target,
        proto_path: substituteVars(g.protoPath, extra).trim(),
        import_paths: importPaths,
        service: substituteVars(g.service, extra).trim(),
        method: substituteVars(g.method, extra).trim(),
        message: substituteVars(g.message, extra).trim() || "{}",
        metadata,
        use_tls: g.useTls,
        authority: substituteVars(g.authority, extra).trim() || null,
      },
    });
    const status = res.status === 0 ? 200 : 500;
    const errText = res.status === 0 ? undefined : `${res.status} ${res.status_text}`;
    return {
      url: summaryUrl,
      response: {
        status,
        headers: res.headers.map((h) => ({ ...h })),
        body: res.body,
        elapsedMs: res.elapsed_ms,
        sizeBytes: res.size_bytes,
        lang: "JSON",
        error: errText,
      },
    };
  }

  const url = buildFlowUrl(req, extra);
  const headers = buildFlowHeaders(req, extra);
  const body = buildFlowBody(req, extra);
  const payload = {
    method: req.method,
    url,
    headers,
    body_type: req.bodyType,
    body,
    form_data: req.bodyType === "form-data" ? buildFlowFormData(req, extra) : [],
    binary_path: req.bodyType === "binary" ? req.binaryPath : undefined,
  };
  const probe = await invoke<HttpResponsePayload>("send_http_request", { payload });
  const ct = probe.headers.find((h) => h.key.toLowerCase() === "content-type")?.value ?? "";
  const lang: ResponseLang = ct.includes("json")
    ? "JSON"
    : ct.includes("xml")
      ? "XML"
      : ct.includes("html")
        ? "HTML"
        : "Text";
  return {
    url,
    response: {
      status: probe.status,
      headers: probe.headers.map((h) => ({ ...h })),
      body: probe.body,
      elapsedMs: probe.elapsed_ms,
      sizeBytes: probe.size_bytes,
      lang,
    },
  };
}

/**
 * Run a user-provided JS snippet as an "extract" step: it has access to
 * `pm.response` (via the same shape used by test scripts) and `ctx`, where
 * `ctx.set('name', value)` writes to the flow's variable bag.
 */
function runExtractScript(
  script: string,
  response: HistoryResponse,
  ctx: {
    vars: Record<string, string>;
    iteration: number;
    row?: FlowDataRow;
  },
): { error?: string } {
  if (!script.trim()) return {};
  try {
    const pm = buildPmForResponse(response);
    const ctxApi = {
      set(name: string, value: unknown) {
        if (typeof name !== "string" || !name.trim()) return;
        ctx.vars[name] =
          value == null ? "" : typeof value === "string" ? value : JSON.stringify(value);
      },
      get(name: string): string | undefined {
        return ctx.vars[name];
      },
      vars: ctx.vars,
      iteration: ctx.iteration,
      row: ctx.row?.values ?? {},
    };
    const fn = new Function("pm", "ctx", '"use strict";\n' + script);
    fn(pm, ctxApi);
    return {};
  } catch (err) {
    return { error: err instanceof Error ? err.message : String(err) };
  }
}

/**
 * Fixed-size worker pool. Runs `worker(item, index)` on each item with at most
 * `size` concurrent in flight. Results preserve input order; failures resolve
 * to a rejected value wrapped by the worker itself (we don't catch here).
 */
async function promisePool<T, R>(
  items: T[],
  size: number,
  worker: (item: T, index: number) => Promise<R>,
): Promise<R[]> {
  const results: R[] = new Array(items.length);
  const cap = Math.max(1, Math.min(size, items.length));
  let cursor = 0;
  const runners: Promise<void>[] = [];
  for (let w = 0; w < cap; w++) {
    runners.push(
      (async () => {
        while (true) {
          const idx = cursor++;
          if (idx >= items.length) break;
          results[idx] = await worker(items[idx], idx);
        }
      })(),
    );
  }
  await Promise.all(runners);
  return results;
}

async function runFlow(flow: FlowRecord): Promise<FlowRunSummary> {
  const startedAt = Date.now();
  const iterations: FlowIterationResult[] = [];
  let passed = 0;
  let failed = 0;
  const requestOf = (id: string) => requests.value.find((r) => r.id === id) ?? null;

  if (flow.mode === "sequence") {
    const iterStart = Date.now();
    const stepResults: FlowStepResult[] = [];
    const localVars: Record<string, string> = {};
    let iterFailed = false;
    for (let i = 0; i < flow.steps.length; i++) {
      const step = flow.steps[i];
      const req = requestOf(step.requestId);
      if (!req) {
        stepResults.push({
          stepId: step.id,
          requestId: step.requestId,
          requestName: step.name || "(缺失请求)",
          ok: false,
          error: "引用的请求已不存在",
        });
        iterFailed = true;
        break;
      }
      if (step.skipIf && step.skipIf.trim()) {
        try {
          const skip = new Function("vars", '"use strict"; return (' + step.skipIf + ")")(
            localVars,
          );
          if (skip) {
            stepResults.push({
              stepId: step.id,
              requestId: req.id,
              requestName: step.name || req.name,
              ok: true,
              error: undefined,
              extractError: "(skipped)",
            });
            continue;
          }
        } catch (err) {
          stepResults.push({
            stepId: step.id,
            requestId: req.id,
            requestName: step.name || req.name,
            ok: false,
            error: `skipIf 表达式错误: ${err instanceof Error ? err.message : String(err)}`,
          });
          iterFailed = true;
          break;
        }
      }
      try {
        const { response } = await executeRequestHeadless(req, localVars);
        const extract = runExtractScript(step.extractScript, response, {
          vars: localVars,
          iteration: 0,
        });
        const ok = response.status != null && response.status < 400 && !extract.error;
        stepResults.push({
          stepId: step.id,
          requestId: req.id,
          requestName: step.name || req.name,
          ok,
          status: response.status ?? undefined,
          elapsedMs: response.elapsedMs ?? undefined,
          sizeBytes: response.sizeBytes ?? undefined,
          error: response.error,
          extractError: extract.error,
          response,
        });
        if (!ok) {
          iterFailed = true;
          break;
        }
      } catch (err) {
        stepResults.push({
          stepId: step.id,
          requestId: req.id,
          requestName: step.name || req.name,
          ok: false,
          error: err instanceof Error ? err.message : String(err),
        });
        iterFailed = true;
        break;
      }
    }
    const iterEnd = Date.now();
    iterations.push({
      iteration: 0,
      startedAt: iterStart,
      finishedAt: iterEnd,
      totalMs: iterEnd - iterStart,
      steps: stepResults,
      finalVars: localVars,
    });
    if (iterFailed) failed++;
    else passed++;
  } else {
    const step = flow.steps[0];
    const req = step ? requestOf(step.requestId) : null;
    if (!step || !req) {
      const now = Date.now();
      return {
        flowId: flow.id,
        flowName: flow.name,
        mode: flow.mode,
        iterations: [],
        totalMs: 0,
        passed: 0,
        failed: 1,
        startedAt,
        finishedAt: now,
      };
    }
    const jobs: { idx: number; row?: FlowDataRow }[] =
      flow.mode === "concurrent"
        ? Array.from({ length: Math.max(1, flow.iterations) }, (_, i) => ({ idx: i }))
        : flow.dataRows.map((row, i) => ({ idx: i, row }));
    const pooled = await promisePool(jobs, flow.concurrency || 1, async (job) => {
      const iterStart = Date.now();
      const localVars: Record<string, string> = { ...(job.row?.values ?? {}) };
      let result: FlowStepResult;
      try {
        const { response } = await executeRequestHeadless(req, localVars);
        const extract = runExtractScript(step.extractScript, response, {
          vars: localVars,
          iteration: job.idx,
          row: job.row,
        });
        const ok = response.status != null && response.status < 400 && !extract.error;
        result = {
          stepId: step.id,
          requestId: req.id,
          requestName: step.name || req.name,
          ok,
          status: response.status ?? undefined,
          elapsedMs: response.elapsedMs ?? undefined,
          sizeBytes: response.sizeBytes ?? undefined,
          error: response.error,
          extractError: extract.error,
          response,
        };
      } catch (err) {
        result = {
          stepId: step.id,
          requestId: req.id,
          requestName: step.name || req.name,
          ok: false,
          error: err instanceof Error ? err.message : String(err),
        };
      }
      const iterEnd = Date.now();
      return {
        iteration: job.idx,
        startedAt: iterStart,
        finishedAt: iterEnd,
        totalMs: iterEnd - iterStart,
        steps: [result],
        finalVars: localVars,
        rowLabel: job.row
          ? Object.entries(job.row.values)
              .map(([k, v]) => `${k}=${v}`)
              .join(", ")
          : undefined,
      } as FlowIterationResult;
    });
    for (const iter of pooled) {
      iterations.push(iter);
      if (iter.steps.every((s) => s.ok)) passed++;
      else failed++;
    }
  }

  const finishedAt = Date.now();
  return {
    flowId: flow.id,
    flowName: flow.name,
    mode: flow.mode,
    iterations,
    totalMs: finishedAt - startedAt,
    passed,
    failed,
    startedAt,
    finishedAt,
  };
}

// ---------- Flow CRUD ----------

const activeFlow = computed<FlowRecord | null>(
  () => flows.value.find((f) => f.id === activeFlowId.value) ?? null,
);

const filteredFlows = computed<FlowRecord[]>(() => {
  const q = flowSearchQuery.value.trim().toLowerCase();
  if (!q) return flows.value;
  return flows.value.filter((f) => f.name.toLowerCase().includes(q));
});

const flowModeLabels: Record<FlowMode, string> = {
  sequence: "链式",
  concurrent: "并发",
  "data-driven": "参数化",
};

function createFlow() {
  const now = Date.now();
  const flow: FlowRecord = {
    id: uuid(),
    name: `新流程 ${flows.value.length + 1}`,
    mode: "sequence",
    steps: [],
    iterations: 1,
    concurrency: 5,
    dataColumns: [],
    dataRows: [],
    createdAt: now,
    updatedAt: now,
  };
  flows.value.push(flow);
  activeFlowId.value = flow.id;
  sidebarTarget.value = "flows";
  schedulePersist();
}

async function deleteFlow(flow: FlowRecord) {
  const confirmed = await openConfirm({
    title: "删除流程",
    message: `确定删除流程「${flow.name}」吗？`,
    confirmText: "删除",
    danger: true,
  });
  if (!confirmed) return;
  flows.value = flows.value.filter((f) => f.id !== flow.id);
  if (activeFlowId.value === flow.id) activeFlowId.value = flows.value[0]?.id ?? "";
  schedulePersist();
}

function renameFlow(flow: FlowRecord, name: string) {
  const trimmed = name.trim();
  if (!trimmed || trimmed === flow.name) return;
  flow.name = trimmed;
  flow.updatedAt = Date.now();
  schedulePersist();
}

function addFlowStep(flow: FlowRecord) {
  const firstReqId = requests.value[0]?.id ?? "";
  flow.steps.push({
    id: uuid(),
    requestId: firstReqId,
    extractScript: "",
  });
  flow.updatedAt = Date.now();
  schedulePersist();
}

function removeFlowStep(flow: FlowRecord, stepId: string) {
  flow.steps = flow.steps.filter((s) => s.id !== stepId);
  flow.updatedAt = Date.now();
  schedulePersist();
}

function moveFlowStep(flow: FlowRecord, stepId: string, delta: number) {
  const idx = flow.steps.findIndex((s) => s.id === stepId);
  if (idx < 0) return;
  const target = idx + delta;
  if (target < 0 || target >= flow.steps.length) return;
  const [removed] = flow.steps.splice(idx, 1);
  flow.steps.splice(target, 0, removed);
  flow.updatedAt = Date.now();
  schedulePersist();
}

function addFlowDataColumn(flow: FlowRecord) {
  const name = `col${flow.dataColumns.length + 1}`;
  flow.dataColumns.push(name);
  for (const row of flow.dataRows) if (!(name in row.values)) row.values[name] = "";
  flow.updatedAt = Date.now();
  schedulePersist();
}

function renameFlowDataColumn(flow: FlowRecord, index: number, newName: string) {
  const trimmed = newName.trim();
  if (!trimmed) return;
  const old = flow.dataColumns[index];
  if (old === trimmed) return;
  if (flow.dataColumns.includes(trimmed)) return;
  flow.dataColumns[index] = trimmed;
  for (const row of flow.dataRows) {
    row.values[trimmed] = row.values[old] ?? "";
    delete row.values[old];
  }
  flow.updatedAt = Date.now();
  schedulePersist();
}

function removeFlowDataColumn(flow: FlowRecord, index: number) {
  const name = flow.dataColumns[index];
  flow.dataColumns.splice(index, 1);
  for (const row of flow.dataRows) delete row.values[name];
  flow.updatedAt = Date.now();
  schedulePersist();
}

function addFlowDataRow(flow: FlowRecord) {
  const values: Record<string, string> = {};
  for (const col of flow.dataColumns) values[col] = "";
  flow.dataRows.push({ id: uuid(), values });
  flow.updatedAt = Date.now();
  schedulePersist();
}

function removeFlowDataRow(flow: FlowRecord, rowId: string) {
  flow.dataRows = flow.dataRows.filter((r) => r.id !== rowId);
  flow.updatedAt = Date.now();
  schedulePersist();
}

function importFlowDataCsv(flow: FlowRecord, csv: string) {
  const lines = csv
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter(Boolean);
  if (!lines.length) return;
  const parseRow = (line: string): string[] => {
    const out: string[] = [];
    let cur = "";
    let inQuote = false;
    for (let i = 0; i < line.length; i++) {
      const ch = line[i];
      if (inQuote) {
        if (ch === '"' && line[i + 1] === '"') {
          cur += '"';
          i++;
        } else if (ch === '"') {
          inQuote = false;
        } else {
          cur += ch;
        }
      } else if (ch === ",") {
        out.push(cur);
        cur = "";
      } else if (ch === '"') {
        inQuote = true;
      } else {
        cur += ch;
      }
    }
    out.push(cur);
    return out;
  };
  const header = parseRow(lines[0]).map((c) => c.trim());
  flow.dataColumns = header;
  flow.dataRows = lines.slice(1).map((line) => {
    const cells = parseRow(line);
    const values: Record<string, string> = {};
    header.forEach((col, i) => {
      values[col] = cells[i] ?? "";
    });
    return { id: uuid(), values };
  });
  flow.updatedAt = Date.now();
  schedulePersist();
}

async function triggerFlowRun() {
  const flow = activeFlow.value;
  if (!flow || flowRunning.value) return;
  if (!flow.steps.length) {
    errorMessage.value = "请先添加至少一个步骤";
    return;
  }
  flowRunning.value = true;
  flowRunSummary.value = null;
  flowExpandedIterations.value = {};
  try {
    const summary = await runFlow(flow);
    flowRunSummary.value = summary;
  } catch (err) {
    errorMessage.value = `流程执行失败: ${err instanceof Error ? err.message : String(err)}`;
  } finally {
    flowRunning.value = false;
  }
}

function selectFlow(id: string) {
  activeFlowId.value = id;
  flowRunSummary.value = null;
  schedulePersist();
}

function prettifyBody() {
  const req = activeRequest.value;
  if (!req || !req.body) return;
  if (req.rawLang === "JSON") {
    try {
      req.body = JSON.stringify(JSON.parse(req.body), null, 2);
    } catch {
      // leave as-is
    }
  }
}

function onBodyPaste(event: ClipboardEvent) {
  const req = activeRequest.value;
  if (!req) return;
  const pasted = event.clipboardData?.getData("text") ?? "";
  const formatted = tryFormatJson(pasted);
  if (formatted === null) return;

  event.preventDefault();

  const ta = bodyTextareaEl.value;
  const start = ta?.selectionStart ?? req.body.length;
  const end = ta?.selectionEnd ?? req.body.length;
  const before = req.body.slice(0, start);
  const after = req.body.slice(end);
  req.body = before + formatted + after;
  if (req.rawLang !== "JSON") req.rawLang = "JSON";
  bodyJsonCollapsed.value[req.id] = [];

  const caret = before.length + formatted.length;
  nextTick(() => {
    if (!ta) return;
    ta.focus();
    ta.selectionStart = ta.selectionEnd = caret;
    syncBodyScroll();
  });
}

const bodyTextareaEl = ref<HTMLTextAreaElement | null>(null);
const bodyHighlightEl = ref<HTMLPreElement | null>(null);
const responseCodeEl = ref<HTMLElement | null>(null);
const responseGutterEl = ref<HTMLElement | null>(null);
let bodySelectionDragFrame = 0;
let bodySelectionPointerX = 0;
let isBodySelectionDragging = false;

function syncBodyScroll() {
  const t = bodyTextareaEl.value;
  const p = bodyHighlightEl.value;
  if (!t || !p) return;
  p.scrollTop = t.scrollTop;
  p.scrollLeft = t.scrollLeft;
}

function syncResponseGutterScroll() {
  const code = responseCodeEl.value;
  const gutter = responseGutterEl.value;
  if (!code || !gutter) return;
  gutter.scrollTop = code.scrollTop;
}

function onResponseGutterWheel(event: WheelEvent) {
  const code = responseCodeEl.value;
  if (!code) return;
  if (event.deltaY) {
    code.scrollTop += event.deltaY;
    syncResponseGutterScroll();
  }
}

function stopBodySelectionDrag() {
  isBodySelectionDragging = false;
  if (bodySelectionDragFrame) {
    cancelAnimationFrame(bodySelectionDragFrame);
    bodySelectionDragFrame = 0;
  }
  window.removeEventListener("mousemove", onBodySelectionMouseMove);
  window.removeEventListener("mouseup", stopBodySelectionDrag);
}

function tickBodySelectionDrag() {
  const t = bodyTextareaEl.value;
  if (!isBodySelectionDragging || !t) return;

  const rect = t.getBoundingClientRect();
  const threshold = 36;
  let deltaX = 0;

  if (bodySelectionPointerX > rect.right - threshold) {
    deltaX = Math.min(24, Math.max(4, bodySelectionPointerX - (rect.right - threshold)) / 2);
  } else if (bodySelectionPointerX < rect.left + threshold) {
    deltaX = -Math.min(24, Math.max(4, rect.left + threshold - bodySelectionPointerX) / 2);
  }

  if (deltaX) {
    t.scrollLeft += deltaX;
  }
  syncBodyScroll();
  bodySelectionDragFrame = requestAnimationFrame(tickBodySelectionDrag);
}

function onBodySelectionMouseMove(event: MouseEvent) {
  bodySelectionPointerX = event.clientX;
}

function startBodySelectionDrag(event: MouseEvent) {
  if (event.button !== 0) return;
  stopBodySelectionDrag();
  isBodySelectionDragging = true;
  bodySelectionPointerX = event.clientX;
  window.addEventListener("mousemove", onBodySelectionMouseMove);
  window.addEventListener("mouseup", stopBodySelectionDrag);
  bodySelectionDragFrame = requestAnimationFrame(tickBodySelectionDrag);
}

const bodyJsonFoldView = computed(() => {
  const req = activeRequest.value;
  if (!req || req.bodyType !== "raw" || req.rawLang !== "JSON") {
    return emptyJsonFoldView(req?.body ?? "");
  }
  return buildJsonFoldView(req.body, new Set(bodyJsonCollapsed.value[req.id] ?? []));
});

const bodyHasJsonFolds = computed(
  () => bodyJsonFoldView.value.valid && (bodyJsonCollapsed.value[activeRequest.value?.id ?? ""] ?? []).length > 0,
);

const bodyHighlighted = computed(() => {
  const req = activeRequest.value;
  const text = req?.body ?? "";
  const lang = req?.rawLang ?? "Text";
  if (!text) return "";
  if (lang === "JSON" && bodyJsonFoldView.value.valid) {
    return `${highlightFoldableJson(bodyJsonFoldView.value)}\n`;
  }
  const html = lang === "JSON" ? highlightJson(text) : escapeHtml(text);
  return `${html}\n`;
});

const canPrettify = computed(
  () =>
    activeRequest.value?.bodyType === "raw" &&
    activeRequest.value?.rawLang === "JSON" &&
    !!activeRequest.value?.body,
);

function clearBody() {
  const req = activeRequest.value;
  if (!req) return;
  if (req.bodyType === "form-data" || req.bodyType === "x-www-form-urlencoded") {
    req.bodyRows = [emptyBodyRow()];
  } else if (req.bodyType === "binary") {
    req.binaryPath = "";
  } else {
    req.body = "";
  }
  bodyJsonCollapsed.value[req.id] = [];
}

const displayedResponse = computed(() => {
  if (!responseBody.value) return "";
  if (responseFormat.value === "raw") return responseBody.value;
  if (responseLang.value === "JSON") {
    try {
      return JSON.stringify(JSON.parse(responseBody.value), null, 2);
    } catch {
      return responseBody.value;
    }
  }
  return responseBody.value;
});

const responseJsonFoldView = computed(() => {
  if (responseFormat.value !== "pretty" || responseLang.value !== "JSON") {
    return emptyJsonFoldView(displayedResponse.value);
  }
  return buildJsonFoldView(displayedResponse.value, new Set(responseJsonCollapsed.value));
});

const responseLineNumbers = computed<JsonFoldLine[]>(() => {
  const view = responseJsonFoldView.value;
  if (view.valid && view.lines.length) return view.lines;
  const text = displayedResponse.value;
  const count = text ? text.split("\n").length : 1;
  return Array.from({ length: Math.max(count, 1) }, (_, i) => ({
    text: "",
    sourceLine: i + 1,
  }));
});

const bodyLineNumbers = computed<JsonFoldLine[]>(() => {
  const view = bodyJsonFoldView.value;
  if (view.valid && view.lines.length) return view.lines;
  const text = activeRequest.value?.body ?? "";
  const count = text ? text.split("\n").length : 1;
  return Array.from({ length: Math.max(count, 1) }, (_, i) => ({
    text: "",
    sourceLine: i + 1,
  }));
});

const grpcMessageHighlighted = computed(() => {
  const text = activeRequest.value?.grpc.message ?? "";
  if (!text) return "";
  return `${highlightJson(text)}\n`;
});

const grpcMessageLineNumbers = computed(() => {
  const text = activeRequest.value?.grpc.message ?? "";
  const count = text ? text.split("\n").length : 1;
  return Array.from({ length: Math.max(count, 1) }, (_, i) => i + 1);
});

function prettifyGrpcMessage() {
  const req = activeRequest.value;
  if (!req || !req.grpc.message) return;
  try {
    req.grpc.message = JSON.stringify(JSON.parse(req.grpc.message), null, 2);
  } catch {
    // leave as-is
  }
}

const protoCache = ref<Record<string, ProtoParseEntry>>({});

function protoCacheKey(req: RequestRecord | null): string {
  if (!req || req.kind !== "grpc") return "";
  const path = req.grpc.protoPath.trim();
  if (!path) return "";
  const dirs = req.grpc.importPaths
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter(Boolean)
    .sort()
    .join("|");
  return `${path}::${dirs}`;
}

async function refreshProtoServices(req: RequestRecord) {
  const key = protoCacheKey(req);
  if (!key) return;
  const existing = protoCache.value[key];
  if (existing && (existing.loading || existing.services || existing.error)) {
    return;
  }
  protoCache.value = {
    ...protoCache.value,
    [key]: { loading: true, services: null, error: "" },
  };
  try {
    const services = await invoke<ProtoServiceDescriptor[]>("parse_proto_services", {
      payload: {
        proto_path: req.grpc.protoPath.trim(),
        import_paths: req.grpc.importPaths
          .split(/\r?\n/)
          .map((l) => l.trim())
          .filter(Boolean),
      },
    });
    protoCache.value = {
      ...protoCache.value,
      [key]: { loading: false, services, error: "" },
    };
    syncServiceMethodSelection(req, services);
  } catch (err) {
    protoCache.value = {
      ...protoCache.value,
      [key]: { loading: false, services: null, error: String(err) },
    };
  }
}

function isMessageBlank(msg: string): boolean {
  const trimmed = msg.trim();
  if (!trimmed) return true;
  if (trimmed === "{}") return true;
  try {
    const v = JSON.parse(trimmed);
    return v && typeof v === "object" && !Array.isArray(v) && Object.keys(v).length === 0;
  } catch {
    return false;
  }
}

function applyMessageTemplate(req: RequestRecord, template: string | undefined, force = false) {
  if (!template) return;
  if (!force && !isMessageBlank(req.grpc.message)) return;
  req.grpc.message = template;
}

function syncServiceMethodSelection(
  req: RequestRecord,
  services: ProtoServiceDescriptor[],
) {
  if (!services.length) return;
  let current = services.find((s) => s.name === req.grpc.service);
  if (!current) {
    req.grpc.service = services[0].name;
    req.grpc.method = services[0].methods[0]?.name ?? "";
    current = services[0];
  } else if (!current.methods.find((m) => m.name === req.grpc.method)) {
    req.grpc.method = current.methods[0]?.name ?? "";
  }
  const selected = current.methods.find((m) => m.name === req.grpc.method);
  applyMessageTemplate(req, selected?.request_template);
}

const activeProtoEntry = computed<ProtoParseEntry | null>(() => {
  const key = protoCacheKey(activeRequest.value);
  if (!key) return null;
  return protoCache.value[key] ?? null;
});

const activeProtoServices = computed<ProtoServiceDescriptor[]>(
  () => activeProtoEntry.value?.services ?? [],
);

const activeProtoMethods = computed<ProtoMethodDescriptor[]>(() => {
  const req = activeRequest.value;
  if (!req || req.kind !== "grpc") return [];
  const svc = activeProtoServices.value.find((s) => s.name === req.grpc.service);
  return svc?.methods ?? [];
});

let protoDebounceTimer: number | null = null;
watch(
  () => {
    const req = activeRequest.value;
    if (!req || req.kind !== "grpc") return "";
    return protoCacheKey(req);
  },
  (key) => {
    if (protoDebounceTimer !== null) window.clearTimeout(protoDebounceTimer);
    if (!key) return;
    protoDebounceTimer = window.setTimeout(() => {
      const req = activeRequest.value;
      if (!req || req.kind !== "grpc") return;
      if (protoCacheKey(req) !== key) return;
      refreshProtoServices(req);
    }, 250) as unknown as number;
  },
  { immediate: true },
);

function pickGrpcService(name: string) {
  const req = activeRequest.value;
  if (!req) return;
  req.grpc.service = name;
  const svc = activeProtoServices.value.find((s) => s.name === name);
  if (svc && !svc.methods.find((m) => m.name === req.grpc.method)) {
    req.grpc.method = svc.methods[0]?.name ?? "";
  }
  const picked = svc?.methods.find((m) => m.name === req.grpc.method);
  applyMessageTemplate(req, picked?.request_template);
  closeDropdown();
  grpcServiceSearch.value = "";
}

function pickGrpcMethod(name: string) {
  const req = activeRequest.value;
  if (!req) return;
  req.grpc.method = name;
  const m = activeProtoMethods.value.find((mm) => mm.name === name);
  applyMessageTemplate(req, m?.request_template);
  closeDropdown();
  grpcMethodSearch.value = "";
}

function resetGrpcMessageToTemplate() {
  const req = activeRequest.value;
  if (!req || req.kind !== "grpc") return;
  const m = activeProtoMethods.value.find((mm) => mm.name === req.grpc.method);
  if (!m?.request_template) return;
  applyMessageTemplate(req, m.request_template, true);
}

const grpcServiceSearch = ref("");
const grpcMethodSearch = ref("");
const grpcServiceSearchEl = ref<HTMLInputElement | null>(null);
const grpcMethodSearchEl = ref<HTMLInputElement | null>(null);

function fuzzyMatch(haystack: string, needle: string): boolean {
  if (!needle) return true;
  return haystack.toLowerCase().includes(needle.toLowerCase());
}

const filteredGrpcServices = computed<ProtoServiceDescriptor[]>(() => {
  const q = grpcServiceSearch.value.trim();
  if (!q) return activeProtoServices.value;
  return activeProtoServices.value.filter(
    (s) =>
      fuzzyMatch(s.name, q) ||
      s.methods.some((m) => fuzzyMatch(m.name, q)),
  );
});

const filteredGrpcMethods = computed<ProtoMethodDescriptor[]>(() => {
  const q = grpcMethodSearch.value.trim();
  if (!q) return activeProtoMethods.value;
  return activeProtoMethods.value.filter(
    (m) =>
      fuzzyMatch(m.name, q) ||
      fuzzyMatch(m.input_type, q) ||
      fuzzyMatch(m.output_type, q),
  );
});

watch(openDropdown, (name, prev) => {
  if (prev === "grpcService" && name !== "grpcService") grpcServiceSearch.value = "";
  if (prev === "grpcMethod" && name !== "grpcMethod") grpcMethodSearch.value = "";
  if (name === "grpcService") setTimeout(() => grpcServiceSearchEl.value?.focus(), 20);
  if (name === "grpcMethod") setTimeout(() => grpcMethodSearchEl.value?.focus(), 20);
});

function reparseActiveProto() {
  const req = activeRequest.value;
  if (!req || req.kind !== "grpc") return;
  const key = protoCacheKey(req);
  if (!key) return;
  const next = { ...protoCache.value };
  delete next[key];
  protoCache.value = next;
  refreshProtoServices(req);
}

function shortTypeName(full: string): string {
  if (!full) return "";
  const idx = full.lastIndexOf(".");
  return idx >= 0 ? full.slice(idx + 1) : full;
}

function streamingLabel(m: ProtoMethodDescriptor): string {
  if (m.client_streaming && m.server_streaming) return "双向流（暂不支持发送）";
  if (m.client_streaming) return "客户端流（暂不支持发送）";
  if (m.server_streaming) return "服务端流（暂不支持发送）";
  return "";
}

function streamingShort(m: ProtoMethodDescriptor): string {
  if (m.client_streaming && m.server_streaming) return "↔ stream";
  if (m.client_streaming) return "→ stream";
  if (m.server_streaming) return "← stream";
  return "";
}

const grpcParseStatusClass = computed(() => {
  const req = activeRequest.value;
  if (!req || req.kind !== "grpc") return "muted";
  if (!req.grpc.protoPath.trim()) return "muted";
  const entry = activeProtoEntry.value;
  if (!entry) return "muted";
  if (entry.loading) return "muted";
  if (entry.error) return "invalid";
  if (entry.services && entry.services.length) return "valid";
  return "muted";
});

const grpcParseStatusText = computed(() => {
  const req = activeRequest.value;
  if (!req || req.kind !== "grpc") return "";
  if (!req.grpc.protoPath.trim()) {
    return "尚未选择 .proto，可在「Proto」页指定后自动识别 service / method";
  }
  const entry = activeProtoEntry.value;
  if (!entry) return "等待解析…";
  if (entry.loading) return "正在解析 .proto …";
  if (entry.error) return `解析失败：${entry.error}`;
  const services = entry.services ?? [];
  const total = services.reduce((acc, s) => acc + s.methods.length, 0);
  if (!services.length) return ".proto 中未发现 gRPC service";
  return `已识别 ${services.length} 个 service · 共 ${total} 个 method`;
});

async function pickProtoFile() {
  const req = activeRequest.value;
  if (!req) return;
  try {
    const initialDir = (() => {
      const current = req.grpc.protoPath;
      if (!current) return undefined;
      const idx = Math.max(current.lastIndexOf("/"), current.lastIndexOf("\\"));
      return idx > 0 ? current.slice(0, idx) : undefined;
    })();
    const picked = await openFileDialog({
      title: "选择 .proto 文件",
      multiple: false,
      directory: false,
      defaultPath: initialDir,
      filters: [
        { name: "Protocol Buffers", extensions: ["proto"] },
        { name: "所有文件", extensions: ["*"] },
      ],
    });
    if (typeof picked === "string" && picked) {
      req.grpc.protoPath = picked;
    }
  } catch (err) {
    errorMessage.value = `打开文件选择器失败：${String(err)}`;
  }
}

async function pickBinaryFile(req: RequestRecord) {
  try {
    const picked = await openFileDialog({
      title: "选择 binary 请求体文件",
      multiple: false,
      directory: false,
    });
    if (typeof picked === "string" && picked) {
      req.binaryPath = picked;
    }
  } catch (err) {
    errorMessage.value = `打开文件选择器失败：${String(err)}`;
  }
}

async function pickBodyRowFile(row: BodyFormRow) {
  try {
    const picked = await openFileDialog({
      title: "选择 form-data 文件",
      multiple: false,
      directory: false,
    });
    if (typeof picked === "string" && picked) {
      row.fieldType = "file";
      row.filePath = picked;
      row.fileName = filenameFromPath(picked);
    }
  } catch (err) {
    errorMessage.value = `打开文件选择器失败：${String(err)}`;
  }
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

function highlightJson(text: string): string {
  if (!text) return "";
  const escaped = escapeHtml(text);
  return escaped.replace(
    /("(?:\\u[a-fA-F0-9]{4}|\\[^u]|[^\\"])*")(\s*:)?|(\b(?:true|false|null)\b)|(-?\d+(?:\.\d+)?(?:[eE][+\-]?\d+)?)|([{}\[\],:])/g,
    (match, str, colon, bool, num, punc) => {
      if (str !== undefined) {
        if (colon) {
          return `<span class="key">${str}</span><span class="punc">${colon}</span>`;
        }
        return `<span class="str">${str}</span>`;
      }
      if (bool) return `<span class="bool">${bool}</span>`;
      if (num) return `<span class="num">${num}</span>`;
      if (punc) return `<span class="punc">${punc}</span>`;
      return match;
    },
  );
}

interface JsonContainer {
  path: string;
  start: number;
  end: number;
  open: "{" | "[";
  startLine: number;
  endLine: number;
}

interface JsonFoldLine {
  text: string;
  sourceLine: number;
  foldPath?: string;
  folded?: boolean;
}

interface JsonFoldView {
  valid: boolean;
  display: string;
  lines: JsonFoldLine[];
  tokens: { path: string; open: "{" | "[" }[];
}

function emptyJsonFoldView(text: string): JsonFoldView {
  const rawLines = text ? text.split("\n") : [""];
  return {
    valid: false,
    display: text,
    lines: rawLines.map((line, i) => ({ text: line, sourceLine: i + 1 })),
    tokens: [],
  };
}

function joinJsonPath(parent: string, key: string): string {
  if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(key)) return `${parent}.${key}`;
  return `${parent}[${JSON.stringify(key)}]`;
}

function offsetOfLine(text: string, lineIdx: number): number {
  if (lineIdx <= 0) return 0;
  let seen = 0;
  for (let i = 0; i < text.length; i++) {
    if (text[i] === "\n") {
      seen++;
      if (seen === lineIdx) return i + 1;
    }
  }
  return text.length;
}

function endOfLine(text: string, lineIdx: number): number {
  const start = offsetOfLine(text, lineIdx);
  const nl = text.indexOf("\n", start);
  return nl === -1 ? text.length : nl;
}

function scanJsonContainers(text: string): JsonContainer[] {
  const containers: JsonContainer[] = [];
  const n = text.length;
  let i = 0;

  const skipWs = () => {
    while (
      i < n &&
      (text[i] === " " || text[i] === "\t" || text[i] === "\n" || text[i] === "\r")
    ) {
      i++;
    }
  };

  const skipString = () => {
    i++;
    while (i < n) {
      if (text[i] === "\\") {
        i += 2;
        continue;
      }
      if (text[i] === '"') {
        i++;
        return;
      }
      i++;
    }
  };

  const lineAt = (pos: number) => {
    let line = 0;
    const lim = Math.min(pos, n);
    for (let k = 0; k < lim; k++) {
      if (text[k] === "\n") line++;
    }
    return line;
  };

  const pushContainer = (path: string, open: "{" | "[", start: number, end: number) => {
    const startLine = lineAt(start);
    const endLine = lineAt(Math.max(end - 1, start));
    if (endLine > startLine) {
      containers.push({ path, start, end, open, startLine, endLine });
    }
  };

  const parseValue = (path: string): void => {
    skipWs();
    if (i >= n) return;
    const ch = text[i];
    if (ch === '"') {
      skipString();
      return;
    }
    if (ch === "{") {
      parseObject(path);
      return;
    }
    if (ch === "[") {
      parseArray(path);
      return;
    }
    while (
      i < n &&
      text[i] !== "," &&
      text[i] !== "}" &&
      text[i] !== "]" &&
      text[i] !== " " &&
      text[i] !== "\n" &&
      text[i] !== "\r" &&
      text[i] !== "\t"
    ) {
      i++;
    }
  };

  const parseObject = (path: string) => {
    const start = i;
    i++;
    skipWs();
    if (i < n && text[i] === "}") {
      i++;
      pushContainer(path, "{", start, i);
      return;
    }
    while (i < n) {
      skipWs();
      if (i < n && text[i] === "}") break;
      if (text[i] !== '"') break;
      const keyStart = i;
      skipString();
      let key = "";
      try {
        key = JSON.parse(text.slice(keyStart, i)) as string;
      } catch {
        key = text.slice(keyStart + 1, Math.max(keyStart + 1, i - 1));
      }
      skipWs();
      if (text[i] === ":") i++;
      parseValue(joinJsonPath(path, key));
      skipWs();
      if (text[i] === ",") {
        i++;
        continue;
      }
      if (text[i] === "}") break;
    }
    if (i < n && text[i] === "}") i++;
    pushContainer(path, "{", start, i);
  };

  const parseArray = (path: string) => {
    const start = i;
    i++;
    skipWs();
    if (i < n && text[i] === "]") {
      i++;
      pushContainer(path, "[", start, i);
      return;
    }
    let index = 0;
    while (i < n) {
      skipWs();
      if (i < n && text[i] === "]") break;
      parseValue(`${path}[${index}]`);
      index++;
      skipWs();
      if (text[i] === ",") {
        i++;
        continue;
      }
      if (text[i] === "]") break;
    }
    if (i < n && text[i] === "]") i++;
    pushContainer(path, "[", start, i);
  };

  parseValue("$");
  return containers;
}

function buildJsonFoldView(text: string, collapsed: Set<string>): JsonFoldView {
  if (!text) return emptyJsonFoldView("");
  try {
    JSON.parse(text);
  } catch {
    return emptyJsonFoldView(text);
  }

  const containers = scanJsonContainers(text);
  if (!containers.length) return { ...emptyJsonFoldView(text), valid: true };

  const collapsedNodes = containers.filter((c) => collapsed.has(c.path));
  const isHidden = (lineIdx: number) =>
    collapsedNodes.some((c) => lineIdx > c.startLine && lineIdx <= c.endLine);

  const sourceLines = text.split("\n");
  const tokens: { path: string; open: "{" | "[" }[] = [];
  const lines: JsonFoldLine[] = [];

  for (let i = 0; i < sourceLines.length; i++) {
    if (isHidden(i)) continue;
    const starting = containers.filter((c) => c.startLine === i);
    const foldableHere = starting.sort((a, b) => b.start - a.start)[0];
    if (foldableHere && collapsed.has(foldableHere.path)) {
      const prefix = text.slice(offsetOfLine(text, i), foldableHere.start);
      const suffix = text.slice(foldableHere.end, endOfLine(text, foldableHere.endLine));
      const tokenIdx = tokens.length;
      tokens.push({ path: foldableHere.path, open: foldableHere.open });
      lines.push({
        text: `${prefix}"§FOLD${tokenIdx}§"${suffix}`,
        sourceLine: i + 1,
        foldPath: foldableHere.path,
        folded: true,
      });
    } else {
      lines.push({
        text: sourceLines[i],
        sourceLine: i + 1,
        foldPath: foldableHere?.path,
        folded: false,
      });
    }
  }

  return {
    valid: true,
    display: lines.map((line) => line.text).join("\n"),
    lines,
    tokens,
  };
}

function highlightFoldableJson(view: JsonFoldView): string {
  const html = highlightJson(view.display);
  return html.replace(/<span class="str">"§FOLD(\d+)§"<\/span>/g, (_match, idx) => {
    const token = view.tokens[Number(idx)];
    if (!token) return "";
    const close = token.open === "{" ? "}" : "]";
    return `<span class="json-fold-ph" data-path="${encodeURIComponent(token.path)}">${token.open} … ${close}</span>`;
  });
}

function toggleCollapsedPath(list: string[], path: string): string[] {
  return list.includes(path) ? list.filter((p) => p !== path) : [...list, path];
}

function toggleBodyJsonFold(path: string) {
  const id = activeRequest.value?.id;
  if (!id) return;
  const current = bodyJsonCollapsed.value[id] ?? [];
  bodyJsonCollapsed.value = {
    ...bodyJsonCollapsed.value,
    [id]: toggleCollapsedPath(current, path),
  };
}

function toggleResponseJsonFold(path: string) {
  responseJsonCollapsed.value = toggleCollapsedPath(responseJsonCollapsed.value, path);
}

function expandAllBodyJson() {
  const id = activeRequest.value?.id;
  if (!id) return;
  bodyJsonCollapsed.value = { ...bodyJsonCollapsed.value, [id]: [] };
}

function expandAllResponseJson() {
  responseJsonCollapsed.value = [];
}

function onJsonFoldClick(event: MouseEvent, toggle: (path: string) => void) {
  const target = event.target as HTMLElement | null;
  const ph = target?.closest(".json-fold-ph") as HTMLElement | null;
  if (!ph?.dataset.path) return;
  toggle(decodeURIComponent(ph.dataset.path));
}

const highlightedResponse = computed(() => {
  const text = displayedResponse.value;
  if (!text) return "";
  if (responseFormat.value === "pretty" && responseLang.value === "JSON") {
    const view = responseJsonFoldView.value;
    if (view.valid) return highlightFoldableJson(view);
    return highlightJson(text);
  }
  return escapeHtml(text);
});

const responsePreviewSrc = computed(() => {
  if (responseLang.value === "HTML") return responseBody.value;
  const fg = isLightTheme.value ? "#1f2128" : "#d4d4d4";
  const bg = isLightTheme.value ? "#fbfbfd" : "#161821";
  return `<pre style="font-family:ui-monospace,Menlo,monospace;color:${fg};background:${bg};padding:12px;margin:0;">${escapeHtml(
    responseBody.value,
  )}</pre>`;
});

async function copyResponse() {
  try {
    await navigator.clipboard.writeText(displayedResponse.value);
  } catch {
    // ignore
  }
}

watch(activeTabId, (id) => {
  if (!id) return;
  const owner = requests.value.find((r) => r.id === id);
  if (owner) selectedFolderId.value = owner.folderId;
});

watch(responseBody, () => {
  responseJsonCollapsed.value = [];
});

watch(highlightedResponse, () => {
  nextTick(syncResponseGutterScroll);
});

watch(
  [
    folders,
    requests,
    environments,
    activeEnvId,
    selectedEnvId,
    openTabIds,
    activeTabId,
    sidebarWidth,
    requestPaneHeight,
    theme,
  ],
  schedulePersist,
  { deep: true },
);

onMounted(async () => {
  await hydrate();
  document.title = appTitle.value;
  void getCurrentWindow().setTitle(appTitle.value);
  window.addEventListener("click", onGlobalClick);
});

onBeforeUnmount(() => {
  window.removeEventListener("click", onGlobalClick);
  window.removeEventListener("mousemove", onDragMove);
  window.removeEventListener("mouseup", onDragEnd);
  stopBodySelectionDrag();
  clearAllRequestTimeouts();
});

const methodLabel = computed(() => activeRequest.value?.method ?? "GET");
const rawLangLabel = computed(() => activeRequest.value?.rawLang ?? "JSON");
const METHOD_OPTIONS: HttpMethod[] = ["GET", "POST", "PUT", "PATCH", "DELETE"];
const RAW_LANG_OPTIONS: RawLang[] = ["Text", "JSON", "XML", "HTML", "JavaScript"];
const RESPONSE_LANG_OPTIONS: ResponseLang[] = ["JSON", "XML", "HTML", "Text"];

const appStyle = computed(
  () => ({ "--sidebar-w": `${sidebarWidth.value}px` } as Record<string, string>),
);
const requestPaneStyle = computed(
  () => ({ height: `${requestPaneHeight.value}px` } as Record<string, string>),
);

function pickMethod(m: HttpMethod) {
  if (activeRequest.value) activeRequest.value.method = m;
  closeDropdown();
}
function pickRawLang(l: RawLang) {
  if (activeRequest.value) activeRequest.value.rawLang = l;
  closeDropdown();
}
function pickResponseLang(l: ResponseLang) {
  responseLang.value = l;
  closeDropdown();
}
function pickImportFolder(id: string) {
  importTargetFolderId.value = id;
  closeDropdown();
}
</script>

<template>
  <div class="app-root">
    <header class="titlebar" data-tauri-drag-region @mousedown="startWindowDrag">
      <button
        class="app-title-button no-window-drag"
        type="button"
        title="点击切换 Postboy / Postgirl"
        @click="toggleAppTitle"
      >
        <img class="app-logo" :src="postboyLogo" alt="" aria-hidden="true" />
        {{ appTitle }}
      </button>
      <div class="title-actions no-window-drag">
        <button class="btn ghost sm" title="导入 cURL" @click="openImportModal">导入</button>
        <div
          class="new-split"
          :class="{ open: openDropdown === 'newTop' }"
        >
          <button
            class="btn primary sm new-main"
            title="新建 HTTP 请求"
            @click="newTab('http')"
          >
            ＋ 新建
          </button>
          <button
            class="btn primary sm new-caret"
            title="选择请求类型"
            @click.stop="toggleDropdown('newTop')"
          >
            <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6" /></svg>
          </button>
          <div v-if="openDropdown === 'newTop'" class="cs-menu align-end new-menu">
            <div class="cs-item" @click.stop="newTab('http')">
              <span class="method get">GET</span>
              <span>HTTP 请求</span>
            </div>
            <div class="cs-item" @click.stop="newTab('grpc')">
              <span class="method grpc">gRPC</span>
              <span>gRPC 请求</span>
            </div>
          </div>
        </div>
        <button
          class="icon-btn theme-toggle"
          :class="{ light: isLightTheme }"
          :title="isLightTheme ? '切换到夜间模式' : '切换到日间模式'"
          @click="toggleTheme"
        >
          <svg
            class="icn sun"
            viewBox="0 0 24 24"
            width="18"
            height="18"
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
            stroke-linejoin="round"
          >
            <circle cx="12" cy="12" r="4" />
            <path
              d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M5.6 18.4 7 17M17 7l1.4-1.4"
            />
          </svg>
          <svg
            class="icn moon"
            viewBox="0 0 24 24"
            width="18"
            height="18"
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
            stroke-linejoin="round"
          >
            <path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z" />
          </svg>
        </button>
        <button class="icon-btn" title="设置">⚙</button>
      </div>
    </header>

    <main class="app" :style="appStyle">
      <nav class="nav-rail" @mousedown="startWindowDrag">
        <button
          class="rail-btn"
          :class="{ active: sidebarTarget === 'collections' }"
          title="集合"
          @click="sidebarTarget = 'collections'"
        >
          <svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
            <path d="M20 17a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2v-6a2 2 0 0 1 2-2h2l2 2h6a2 2 0 0 1 2 2z" />
            <path d="M18 9V7a2 2 0 0 0-2-2H8L6 3H4a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2" />
          </svg>
          <span>集合</span>
        </button>
        <button
          class="rail-btn"
          :class="{ active: sidebarTarget === 'environments' }"
          title="环境"
          @click="sidebarTarget = 'environments'"
        >
          <svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="12" r="9" />
            <ellipse cx="12" cy="12" rx="4" ry="9" />
            <path d="M3 12h18" />
          </svg>
          <span>环境</span>
        </button>
        <button
          class="rail-btn"
          :class="{ active: sidebarTarget === 'history' }"
          title="历史"
          @click="sidebarTarget = 'history'"
        >
          <svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
            <path d="M3 12a9 9 0 1 0 3-6.7L3 8" />
            <path d="M3 3v5h5" />
            <path d="M12 8v4.5l3 2" />
          </svg>
          <span>历史</span>
        </button>
        <button
          class="rail-btn"
          :class="{ active: sidebarTarget === 'flows' }"
          title="Flows"
          @click="sidebarTarget = 'flows'"
        >
          <svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="5" cy="6" r="2" />
            <circle cx="5" cy="18" r="2" />
            <circle cx="19" cy="12" r="2" />
            <path d="M7 6h4a4 4 0 0 1 4 4v0a2 2 0 0 0 2 2" />
            <path d="M7 18h4a4 4 0 0 0 4-4v0a2 2 0 0 1 2-2" />
          </svg>
          <span>Flows</span>
        </button>
        <div class="rail-spacer" data-tauri-drag-region></div>
        <div class="env-indicator no-window-drag" title="当前生效环境">
          <span class="dot" :class="activeEnvDotKind"></span>
          <span>{{ activeEnvLabel }}</span>
        </div>
      </nav>

      <aside class="sidebar" @mousedown="startWindowDrag">
        <section class="panel" :class="{ active: sidebarTarget === 'collections' }">
          <div class="panel-header" :class="{ compact: compactSidebar }">
            <div class="search" :style="searchStyle">
              <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2">
                <circle cx="11" cy="11" r="7" />
                <path d="m21 21-4.3-4.3" />
              </svg>
              <input v-model="searchQuery" type="text" placeholder="搜索集合 / 请求" />
              <button
                v-if="searchQuery"
                class="search-clear"
                title="清空"
                @click="searchQuery = ''"
              >×</button>
            </div>
            <button class="panel-act primary" title="新建集合" @click="addFolder">
              <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
                <path d="M12 5v14M5 12h14" />
              </svg>
            </button>
            <button class="panel-act" title="从 cURL 导入请求" @click="openImportModal">
              <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <path d="M5 5 15 15" />
                <path d="M15 7v8h-8" />
                <path d="M4 20h16" />
              </svg>
            </button>
          </div>

          <div class="tree">
            <div
              v-for="folder in filteredFolders"
              :key="folder.id"
              class="tree-node folder"
              :class="{ open: folder.expanded, selected: selectedFolderId === folder.id }"
            >
              <div class="tree-row" @click="toggleFolder(folder)">
                <span class="caret" :class="{ open: folder.expanded }" aria-hidden="true">
                  <svg viewBox="0 0 24 24" width="10" height="10" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round">
                    <path d="m9 6 6 6-6 6" />
                  </svg>
                </span>
                <svg
                  class="folder-icon"
                  :class="{ open: folder.expanded }"
                  viewBox="0 0 24 24"
                  aria-hidden="true"
                >
                  <path
                    v-if="!folder.expanded"
                    d="M3 7a2 2 0 0 1 2-2h4.17a2 2 0 0 1 1.42.59l1.41 1.41A2 2 0 0 0 13.41 8H19a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"
                  />
                  <template v-else>
                    <path d="M3 7a2 2 0 0 1 2-2h4.17a2 2 0 0 1 1.42.59l1.41 1.41A2 2 0 0 0 13.41 8H19a2 2 0 0 1 2 2v1H3z" />
                    <path d="m3 11 1.6 6.4A2 2 0 0 0 6.54 19h11.32a2 2 0 0 0 1.94-1.6L21 11z" />
                  </template>
                </svg>
                <span class="tree-label">{{ folder.name }}</span>
                <span
                  v-if="selectedFolderId === folder.id"
                  class="folder-selected-tag"
                  title="下次新建请求会落在此集合"
                >当前</span>
                <span class="tree-actions" @click.stop>
                  <button class="icon-btn xs" title="新增请求" @click="addRequestToFolder(folder)">＋</button>
                  <button class="icon-btn xs" title="删除集合" @click="deleteFolder(folder)">✕</button>
                </span>
              </div>
              <div v-show="folder.expanded" class="tree-children">
                <div
                  v-for="r in requestsInFolder(folder.id)"
                  :key="r.id"
                  class="tree-row request"
                  :class="{ active: activeTabId === r.id }"
                  @click="openTab(r.id)"
                >
                  <span class="method" :class="requestTagKey(r)">{{ requestTagText(r) }}</span>
                  <span class="tree-label">{{ r.name }}</span>
                  <span class="tree-actions" @click.stop>
                    <button class="icon-btn xs" title="删除" @click="deleteRequest(r)">✕</button>
                  </span>
                </div>
                <div v-if="!requestsInFolder(folder.id).length" class="tree-empty">空集合</div>
              </div>
            </div>
            <div v-if="!filteredFolders.length" class="tree-empty">暂无集合</div>
          </div>
        </section>

        <section class="panel" :class="{ active: sidebarTarget === 'environments' }">
          <div class="panel-header" :class="{ compact: compactSidebar }">
            <div class="search" :style="searchStyle">
              <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2">
                <circle cx="11" cy="11" r="7" />
                <path d="m21 21-4.3-4.3" />
              </svg>
              <input v-model="envSearchQuery" type="text" placeholder="搜索环境" />
              <button
                v-if="envSearchQuery"
                class="search-clear"
                title="清空"
                @click="envSearchQuery = ''"
              >×</button>
            </div>
            <button class="panel-act primary" title="新建环境" @click="addEnvironment">
              <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
                <path d="M12 5v14M5 12h14" />
              </svg>
            </button>
          </div>
          <div class="env-list">
            <div
              v-for="env in filteredEnvs"
              :key="env.id"
              class="env-item"
              :class="{ active: selectedEnvId === env.id }"
              @click="onClickEnv(env)"
            >
              <span class="dot" :class="env.kind"></span>
              <span class="env-name">{{ env.name }}</span>
              <span v-if="env.kind === 'global'" class="env-tag">KV</span>
              <span v-else-if="activeEnvId === env.id" class="env-tag">激活</span>
            </div>
          </div>

          <div v-if="selectedEnv" class="env-editor">
            <div class="env-editor-head">
              <div class="env-editor-title">
                <span class="dot" :class="selectedEnv.kind"></span>
                <strong>{{ selectedEnv.name }}</strong>
                <span v-if="selectedEnv.kind === 'global'" class="muted">所有环境共享</span>
              </div>
              <button
                v-if="selectedEnv.kind === 'custom'"
                class="icon-btn xs"
                title="删除"
                @click="deleteEnvironment(selectedEnv)"
              >
                ✕
              </button>
            </div>
            <div class="env-section-label">Variables</div>
            <table class="kv-table compact">
              <thead>
                <tr>
                  <th class="check"></th>
                  <th>Key</th>
                  <th>Value</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="(row, i) in selectedEnv.variables" :key="i">
                  <td class="check"><input v-model="row.enabled" type="checkbox" /></td>
                  <td>
                    <input
                      v-model="row.key"
                      type="text"
                      placeholder="key"
                      @input="ensureTrailingRow(selectedEnv.variables)"
                    />
                  </td>
                  <td>
                    <input
                      v-model="row.value"
                      type="text"
                      placeholder="value"
                      @input="ensureTrailingRow(selectedEnv.variables)"
                    />
                  </td>
                  <td><button class="icon-btn xs" @click="removeRow(selectedEnv.variables, i)">×</button></td>
                </tr>
              </tbody>
            </table>
            <div class="env-section-label">
              HTTP Headers
              <span class="muted">随当前激活环境自动附加到 HTTP 请求</span>
            </div>
            <table class="kv-table compact">
              <thead>
                <tr>
                  <th class="check"></th>
                  <th>Header</th>
                  <th>Value</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="(row, i) in selectedEnv.headers" :key="i">
                  <td class="check"><input v-model="row.enabled" type="checkbox" /></td>
                  <td>
                    <input
                      v-model="row.key"
                      type="text"
                      placeholder="Header"
                      @input="ensureTrailingRow(selectedEnv.headers)"
                    />
                  </td>
                  <td>
                    <input
                      v-model="row.value"
                      type="text"
                      placeholder="Value"
                      @input="ensureTrailingRow(selectedEnv.headers)"
                    />
                  </td>
                  <td><button class="icon-btn xs" @click="removeRow(selectedEnv.headers, i)">×</button></td>
                </tr>
              </tbody>
            </table>
          </div>
        </section>

        <section class="panel" :class="{ active: sidebarTarget === 'history' }">
          <div class="panel-header" :class="{ compact: compactSidebar }">
            <div class="search" :style="searchStyle">
              <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2">
                <circle cx="11" cy="11" r="7" />
                <path d="m21 21-4.3-4.3" />
              </svg>
              <input v-model="historySearchQuery" type="text" placeholder="搜索历史（URL / 方法 / 状态码）" />
              <button
                v-if="historySearchQuery"
                class="search-clear"
                title="清空"
                @click="historySearchQuery = ''"
              >×</button>
            </div>
            <button
              class="panel-act danger-hover"
              title="清空历史"
              :disabled="!history.length"
              @click="() => { history.splice(0, history.length); clearHistoryRemote(); }"
            >
              <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <path d="M3 6h18" />
                <path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
                <path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
              </svg>
            </button>
          </div>
          <div class="history-list">
            <div
              v-for="entry in filteredHistory"
              :key="entry.id"
              class="history-item"
              :title="`${entry.method} ${entry.url}`"
              @click="openHistoryEntry(entry)"
            >
              <span class="method" :class="methodKey(entry.method)">{{ methodTagText(entry.method) }}</span>
              <span class="hist-url">{{ entry.url }}</span>
              <span class="hist-status" :class="entry.status && entry.status < 400 ? 'ok' : 'err'">
                {{ entry.status ?? "—" }}
              </span>
            </div>
            <div v-if="!history.length" class="tree-empty">暂无请求历史</div>
            <div v-else-if="!filteredHistory.length" class="tree-empty">没有匹配的历史</div>
          </div>
        </section>

        <section class="panel" :class="{ active: sidebarTarget === 'flows' }">
          <div class="panel-header" :class="{ compact: compactSidebar }">
            <div class="search" :style="searchStyle">
              <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2">
                <circle cx="11" cy="11" r="7" />
                <path d="m21 21-4.3-4.3" />
              </svg>
              <input v-model="flowSearchQuery" type="text" placeholder="搜索流程" />
              <button
                v-if="flowSearchQuery"
                class="search-clear"
                title="清空"
                @click="flowSearchQuery = ''"
              >×</button>
            </div>
            <button class="panel-act" title="新建流程" @click="createFlow">
              <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
                <path d="M12 5v14" />
                <path d="M5 12h14" />
              </svg>
            </button>
          </div>
          <div class="flow-list">
            <div
              v-for="flow in filteredFlows"
              :key="flow.id"
              class="flow-item"
              :class="{ active: activeFlowId === flow.id }"
              :title="flow.name"
              @click="selectFlow(flow.id)"
            >
              <span class="flow-mode-tag" :class="flow.mode">{{ flowModeLabels[flow.mode] }}</span>
              <span class="flow-name">{{ flow.name }}</span>
              <button
                class="flow-del"
                title="删除流程"
                @click.stop="deleteFlow(flow)"
              >×</button>
            </div>
            <div v-if="!flows.length" class="tree-empty">暂无流程，点右上 + 新建</div>
            <div v-else-if="!filteredFlows.length" class="tree-empty">没有匹配的流程</div>
          </div>
        </section>
      </aside>

      <div
        class="v-resizer"
        title="拖动调整侧栏宽度"
        @mousedown="startSidebarDrag"
      ></div>

      <section class="workspace">
        <template v-if="sidebarTarget === 'flows'">
          <div v-if="!activeFlow" class="flow-empty-main">
            <div class="flow-empty-inner">
              <svg viewBox="0 0 24 24" width="48" height="48" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round">
                <circle cx="5" cy="6" r="2" />
                <circle cx="5" cy="18" r="2" />
                <circle cx="19" cy="12" r="2" />
                <path d="M7 6h4a4 4 0 0 1 4 4a2 2 0 0 0 2 2" />
                <path d="M7 18h4a4 4 0 0 0 4-4a2 2 0 0 1 2-2" />
              </svg>
              <p>选择左侧的流程，或点击 + 新建流程</p>
              <button class="btn primary" @click="createFlow">新建流程</button>
            </div>
          </div>
          <div v-else class="flow-editor">
            <div class="flow-editor-header">
              <input
                class="flow-name-input"
                :value="activeFlow.name"
                @change="(e) => renameFlow(activeFlow!, (e.target as HTMLInputElement).value)"
              />
              <div class="flow-mode-picker">
                <button
                  v-for="m in (['sequence','concurrent','data-driven'] as FlowMode[])"
                  :key="m"
                  class="chip-btn"
                  :class="{ active: activeFlow.mode === m }"
                  @click="() => { activeFlow!.mode = m; activeFlow!.updatedAt = Date.now(); schedulePersist(); }"
                >{{ flowModeLabels[m] }}</button>
              </div>
              <div class="flow-header-spacer"></div>
              <button
                class="btn primary"
                :disabled="flowRunning"
                @click="triggerFlowRun"
              >{{ flowRunning ? "运行中…" : "▶ 运行" }}</button>
            </div>

            <div class="flow-editor-body">
              <!-- sequence -->
              <div v-if="activeFlow.mode === 'sequence'" class="flow-steps">
                <div class="flow-hint">
                  链式：按顺序执行每个步骤。用 <code>ctx.set('token', pm.response.json().access_token)</code> 把上一步响应写入变量，后续步骤的 URL / Header / Body 里用 <code v-pre>{{token}}</code> 引用。
                </div>
                <div
                  v-for="(step, idx) in activeFlow.steps"
                  :key="step.id"
                  class="flow-step"
                >
                  <div class="flow-step-head">
                    <span class="flow-step-idx">#{{ idx + 1 }}</span>
                    <select
                      class="flow-step-req"
                      v-model="step.requestId"
                      @change="() => { activeFlow!.updatedAt = Date.now(); schedulePersist(); }"
                    >
                      <option value="" disabled>选择请求…</option>
                      <option
                        v-for="r in requests"
                        :key="r.id"
                        :value="r.id"
                      >{{ r.name }} · {{ r.method }} {{ r.url }}</option>
                    </select>
                    <button class="icon-btn sm" title="上移" :disabled="idx === 0" @click="moveFlowStep(activeFlow!, step.id, -1)">↑</button>
                    <button class="icon-btn sm" title="下移" :disabled="idx === activeFlow.steps.length - 1" @click="moveFlowStep(activeFlow!, step.id, 1)">↓</button>
                    <button class="icon-btn sm danger-hover" title="删除" @click="removeFlowStep(activeFlow!, step.id)">✕</button>
                  </div>
                  <div class="flow-step-body">
                    <label class="flow-step-label">提取脚本 (JS)</label>
                    <textarea
                      class="flow-extract"
                      v-model="step.extractScript"
                      spellcheck="false"
                      placeholder="ctx.set('token', pm.response.json().access_token)"
                      @input="() => { activeFlow!.updatedAt = Date.now(); schedulePersist(); }"
                    ></textarea>
                  </div>
                </div>
                <button class="btn ghost" @click="addFlowStep(activeFlow!)">+ 添加步骤</button>
              </div>

              <!-- concurrent -->
              <div v-else-if="activeFlow.mode === 'concurrent'" class="flow-simple">
                <div class="flow-hint">
                  并发：把选中的请求同时打 N 次，用于快速冒烟或压测。
                </div>
                <label class="flow-field">
                  <span>请求</span>
                  <select
                    v-model="(activeFlow.steps[0] ??= { id: uuid(), requestId: requests[0]?.id ?? '', extractScript: '' }).requestId"
                    @change="() => { activeFlow!.updatedAt = Date.now(); schedulePersist(); }"
                  >
                    <option v-for="r in requests" :key="r.id" :value="r.id">{{ r.name }} · {{ r.method }} {{ r.url }}</option>
                  </select>
                </label>
                <label class="flow-field">
                  <span>迭代次数</span>
                  <input type="number" min="1" max="10000" v-model.number="activeFlow.iterations" @change="() => { activeFlow!.updatedAt = Date.now(); schedulePersist(); }" />
                </label>
                <label class="flow-field">
                  <span>并发度</span>
                  <input type="number" min="1" max="500" v-model.number="activeFlow.concurrency" @change="() => { activeFlow!.updatedAt = Date.now(); schedulePersist(); }" />
                </label>
              </div>

              <!-- data-driven -->
              <div v-else class="flow-simple">
                <div class="flow-hint">
                  参数化：数据表的每一行都会跑一次，行数据以列名作为变量名注入到 URL/Header/Body 的 <code v-pre>{{col}}</code>。
                </div>
                <label class="flow-field">
                  <span>请求</span>
                  <select
                    v-model="(activeFlow.steps[0] ??= { id: uuid(), requestId: requests[0]?.id ?? '', extractScript: '' }).requestId"
                    @change="() => { activeFlow!.updatedAt = Date.now(); schedulePersist(); }"
                  >
                    <option v-for="r in requests" :key="r.id" :value="r.id">{{ r.name }} · {{ r.method }} {{ r.url }}</option>
                  </select>
                </label>
                <label class="flow-field">
                  <span>并发度</span>
                  <input type="number" min="1" max="500" v-model.number="activeFlow.concurrency" @change="() => { activeFlow!.updatedAt = Date.now(); schedulePersist(); }" />
                </label>
                <div class="flow-data-toolbar">
                  <button class="btn ghost sm" @click="addFlowDataColumn(activeFlow!)">+ 列</button>
                  <button class="btn ghost sm" @click="addFlowDataRow(activeFlow!)">+ 行</button>
                  <label class="btn ghost sm">
                    导入 CSV
                    <input
                      type="file"
                      accept=".csv,text/csv"
                      style="display:none"
                      @change="async (e) => {
                        const f = (e.target as HTMLInputElement).files?.[0];
                        if (!f) return;
                        const text = await f.text();
                        importFlowDataCsv(activeFlow!, text);
                        (e.target as HTMLInputElement).value = '';
                      }"
                    />
                  </label>
                </div>
                <div class="flow-data-table-wrap">
                  <table class="flow-data-table">
                    <thead>
                      <tr>
                        <th class="idx">#</th>
                        <th v-for="(col, ci) in activeFlow.dataColumns" :key="ci">
                          <input
                            :value="col"
                            @change="(e) => renameFlowDataColumn(activeFlow!, ci, (e.target as HTMLInputElement).value)"
                          />
                          <button class="col-del" @click="removeFlowDataColumn(activeFlow!, ci)">×</button>
                        </th>
                        <th v-if="!activeFlow.dataColumns.length" class="empty">先添加列</th>
                      </tr>
                    </thead>
                    <tbody>
                      <tr v-for="(row, ri) in activeFlow.dataRows" :key="row.id">
                        <td class="idx">{{ ri + 1 }}</td>
                        <td v-for="col in activeFlow.dataColumns" :key="col">
                          <input
                            v-model="row.values[col]"
                            @change="() => { activeFlow!.updatedAt = Date.now(); schedulePersist(); }"
                          />
                        </td>
                        <td class="row-del">
                          <button @click="removeFlowDataRow(activeFlow!, row.id)">×</button>
                        </td>
                      </tr>
                      <tr v-if="!activeFlow.dataRows.length">
                        <td :colspan="Math.max(1, activeFlow.dataColumns.length + 2)" class="empty">
                          还没有数据行
                        </td>
                      </tr>
                    </tbody>
                  </table>
                </div>
              </div>
            </div>

            <!-- run report -->
            <div v-if="flowRunSummary" class="flow-report">
              <div class="flow-report-head">
                <strong>运行报告</strong>
                <span class="chip ok">通过 {{ flowRunSummary.passed }}</span>
                <span class="chip err" v-if="flowRunSummary.failed">失败 {{ flowRunSummary.failed }}</span>
                <span class="chip">总耗时 {{ flowRunSummary.totalMs }}ms</span>
                <span v-if="flowRunSummary.mode === 'concurrent' && flowRunSummary.totalMs > 0" class="chip">
                  QPS ≈ {{ (flowRunSummary.iterations.length * 1000 / flowRunSummary.totalMs).toFixed(1) }}
                </span>
                <div class="flow-header-spacer"></div>
                <button class="icon-btn" title="关闭" @click="flowRunSummary = null">×</button>
              </div>
              <div class="flow-report-body">
                <table class="flow-report-table">
                  <thead>
                    <tr>
                      <th class="idx">#</th>
                      <th>Step</th>
                      <th>Status</th>
                      <th>Elapsed</th>
                      <th>Size</th>
                      <th>Error</th>
                    </tr>
                  </thead>
                  <tbody>
                    <template v-for="iter in flowRunSummary.iterations" :key="iter.iteration">
                      <tr
                        v-for="(s, si) in iter.steps"
                        :key="`${iter.iteration}-${s.stepId}`"
                        :class="{ err: !s.ok }"
                      >
                        <td class="idx">{{ iter.iteration + 1 }}<span v-if="iter.steps.length > 1">.{{ si + 1 }}</span></td>
                        <td>
                          <div class="step-name">{{ s.requestName }}</div>
                          <div v-if="iter.rowLabel" class="step-sub">{{ iter.rowLabel }}</div>
                        </td>
                        <td>
                          <span v-if="s.status != null" class="status" :class="s.status < 400 ? 'ok' : 'err'">{{ s.status }}</span>
                          <span v-else class="status err">—</span>
                        </td>
                        <td>{{ s.elapsedMs != null ? `${s.elapsedMs}ms` : '—' }}</td>
                        <td>{{ s.sizeBytes != null ? `${s.sizeBytes}B` : '—' }}</td>
                        <td class="err-cell">
                          <span v-if="s.error">{{ s.error }}</span>
                          <span v-else-if="s.extractError">脚本: {{ s.extractError }}</span>
                        </td>
                      </tr>
                    </template>
                  </tbody>
                </table>
              </div>
            </div>
          </div>
        </template>
        <template v-else>
        <div class="tabs">
          <div
            v-for="tab in openTabs"
            :key="tab.id"
            class="tab"
            :class="{ active: activeTabId === tab.id }"
            @click="activeTabId = tab.id"
          >
            <span class="method" :class="requestTagKey(tab)">{{ requestTagText(tab) }}</span>
            <span class="tab-title">{{ tab.name }}</span>
            <button class="tab-close" title="关闭" @click.stop="closeTab(tab.id)" aria-label="关闭">
              <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M18 6 6 18" />
                <path d="m6 6 12 12" />
              </svg>
            </button>
          </div>
          <button class="tab new" title="新建请求" @click="newTab('http')" aria-label="新建请求">
            <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
              <path d="M12 5v14" />
              <path d="M5 12h14" />
            </svg>
          </button>
        </div>

        <template v-if="activeRequest">
          <div class="request-pane" :style="requestPaneStyle">
            <div class="row breadcrumb-row">
              <div class="breadcrumb">
                <span class="crumb">{{ activeFolderName }}</span>
                <span class="sep">/</span>
                <span
                  :key="activeRequest.id"
                  class="crumb editable"
                  contenteditable="true"
                  spellcheck="false"
                  @blur="onRenameRequest($event)"
                  @keydown.enter.prevent="onRenameRequestEnter($event)"
                >{{ activeRequest.name }}</span>
              </div>
              <div class="actions">
                <button class="btn ghost sm" title="保存" @click="persistNow">保存</button>
              </div>
            </div>

            <div v-if="activeRequest.kind === 'http'" class="row request-url-line">
              <div class="url-row">
                <div
                  class="custom-select method-select"
                  :class="[methodKey(activeRequest.method), { open: openDropdown === 'method' }]"
                >
                  <button
                    type="button"
                    class="cs-trigger"
                    @click.stop="toggleDropdown('method')"
                  >
                    <span class="dot-method" :class="methodKey(activeRequest.method)"></span>
                    <span class="cs-value">{{ methodLabel }}</span>
                    <span class="cs-caret">
                      <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <path d="m6 9 6 6 6-6" />
                      </svg>
                    </span>
                  </button>
                  <div v-if="openDropdown === 'method'" class="cs-menu">
                    <div
                      v-for="m in METHOD_OPTIONS"
                      :key="m"
                      class="cs-item"
                      :class="{ active: activeRequest.method === m }"
                      @click.stop="pickMethod(m)"
                    >
                      <span class="dot-method" :class="methodKey(m)"></span>
                      <span>{{ m }}</span>
                    </div>
                  </div>
                </div>
                <div class="url-input">
                  <input
                    v-model="activeRequest.url"
                    type="text"
                    placeholder="输入请求 URL"
                    @keydown.enter="sendRequest"
                  />
                </div>
              </div>
              <div class="send-group" :class="{ open: openDropdown === 'send' }">
                <button class="btn primary" :disabled="isActiveRequestSending" @click="sendRequest">
                  {{ isActiveRequestSending ? "发送中…" : "Send" }}
                </button>
                <button
                  type="button"
                  class="btn primary split"
                  title="更多发送方式"
                  @click.stop="toggleDropdown('send')"
                >
                  <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
                    <path d="m6 9 6 6 6-6" />
                  </svg>
                </button>
                <div v-if="openDropdown === 'send'" class="dropdown">
                  <div
                    class="dropdown-item"
                    @click="closeDropdown(); sendRequest()"
                  >
                    <span class="dd-icon">
                      <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M7 17 17 7" />
                        <path d="M8 7h9v9" />
                      </svg>
                    </span>
                    <span>Send</span>
                    <span class="kbd">⌘↵</span>
                  </div>
                  <div class="dropdown-item disabled">
                    <span class="dd-icon">
                      <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M12 3v12" />
                        <path d="m7 10 5 5 5-5" />
                        <path d="M5 21h14" />
                      </svg>
                    </span>
                    <span>Send and Download</span>
                    <span class="kbd">⌘⇧↵</span>
                  </div>
                </div>
              </div>
            </div>

            <div v-else class="row request-url-line">
              <div class="url-row grpc">
                <span class="grpc-tag">gRPC</span>
                <input
                  v-model="activeRequest.grpc.target"
                  type="text"
                  class="grpc-target-input"
                  placeholder="target，例如 localhost:50051"
                  @keydown.enter="sendRequest"
                />
              </div>
              <label class="grpc-tls" title="使用 TLS（HTTPS）连接">
                <input v-model="activeRequest.grpc.useTls" type="checkbox" />
                <span>TLS</span>
              </label>
              <div class="send-group" :class="{ open: openDropdown === 'send' }">
                <button class="btn primary" :disabled="isActiveRequestSending" @click="sendRequest">
                  {{ isActiveRequestSending ? "发送中…" : "Send" }}
                </button>
                <button
                  type="button"
                  class="btn primary split"
                  title="更多发送方式"
                  @click.stop="toggleDropdown('send')"
                >
                  <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
                    <path d="m6 9 6 6 6-6" />
                  </svg>
                </button>
                <div v-if="openDropdown === 'send'" class="dropdown">
                  <div
                    class="dropdown-item"
                    @click="closeDropdown(); sendRequest()"
                  >
                    <span class="dd-icon">
                      <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M7 17 17 7" />
                        <path d="M8 7h9v9" />
                      </svg>
                    </span>
                    <span>Send</span>
                    <span class="kbd">⌘↵</span>
                  </div>
                  <div class="dropdown-item disabled">
                    <span class="dd-icon">
                      <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M12 3v12" />
                        <path d="m7 10 5 5 5-5" />
                        <path d="M5 21h14" />
                      </svg>
                    </span>
                    <span>Send and Download</span>
                    <span class="kbd">⌘⇧↵</span>
                  </div>
                </div>
              </div>
            </div>

            <div v-if="activeRequest.kind === 'http'" class="param-tabs">
              <div
                class="param-tab"
                :class="{ active: requestTab === 'params' }"
                @click="requestTab = 'params'"
              >
                Params
                <span v-if="paramsCount" class="badge">{{ paramsCount }}</span>
              </div>
              <div
                class="param-tab"
                :class="{ active: requestTab === 'headers' }"
                @click="requestTab = 'headers'"
              >
                Headers
                <span v-if="headersCount" class="badge">{{ headersCount }}</span>
              </div>
              <div
                class="param-tab"
                :class="{ active: requestTab === 'body' }"
                @click="requestTab = 'body'"
              >
                Body
              </div>
              <div
                class="param-tab"
                :class="{ active: requestTab === 'auth' }"
                @click="requestTab = 'auth'"
              >
                Authorization
              </div>
              <div
                class="param-tab"
                :class="{ active: requestTab === 'tests' }"
                @click="requestTab = 'tests'"
              >
                Tests
                <span v-if="testsLineCount" class="badge">{{ testsLineCount }}</span>
              </div>
            </div>

            <template v-if="activeRequest.kind === 'http'">
            <div v-if="requestTab === 'params'" class="param-content active">
              <table class="kv-table">
                <thead>
                  <tr>
                    <th class="check"></th>
                    <th>Key</th>
                    <th>Value</th>
                    <th>Description</th>
                    <th></th>
                  </tr>
                </thead>
                <tbody>
                  <tr v-for="(row, i) in activeRequest.params" :key="i">
                    <td class="check"><input v-model="row.enabled" type="checkbox" /></td>
                    <td>
                      <input
                        v-model="row.key"
                        type="text"
                        placeholder="Key"
                        @input="ensureTrailingRow(activeRequest.params)"
                      />
                    </td>
                    <td>
                      <input
                        v-model="row.value"
                        type="text"
                        placeholder="Value"
                        @input="ensureTrailingRow(activeRequest.params)"
                      />
                    </td>
                    <td>
                      <input v-model="row.description" type="text" placeholder="Description" />
                    </td>
                    <td><button class="icon-btn xs" @click="removeRow(activeRequest.params, i)">×</button></td>
                  </tr>
                </tbody>
              </table>
            </div>

            <div v-else-if="requestTab === 'headers'" class="param-content active">
              <table class="kv-table">
                <thead>
                  <tr>
                    <th class="check"></th>
                    <th>Key</th>
                    <th>Value</th>
                    <th>Description</th>
                    <th></th>
                  </tr>
                </thead>
                <tbody>
                  <tr v-for="(row, i) in activeRequest.headers" :key="i">
                    <td class="check"><input v-model="row.enabled" type="checkbox" /></td>
                    <td>
                      <input
                        v-model="row.key"
                        type="text"
                        placeholder="Header"
                        @input="ensureTrailingRow(activeRequest.headers)"
                      />
                    </td>
                    <td>
                      <input
                        v-model="row.value"
                        type="text"
                        placeholder="Value"
                        @input="ensureTrailingRow(activeRequest.headers)"
                      />
                    </td>
                    <td>
                      <input v-model="row.description" type="text" placeholder="Description" />
                    </td>
                    <td><button class="icon-btn xs" @click="removeRow(activeRequest.headers, i)">×</button></td>
                  </tr>
                </tbody>
              </table>
            </div>

            <div v-else-if="requestTab === 'body'" class="param-content active">
              <div class="body-types">
                <label v-for="opt in BODY_TYPES" :key="opt" class="radio">
                  <input v-model="activeRequest.bodyType" type="radio" :value="opt" />
                  {{ opt }}
                </label>
                <div
                  v-if="activeRequest.bodyType === 'raw'"
                  class="custom-select raw-select"
                  :class="{ open: openDropdown === 'rawLang' }"
                >
                  <button
                    type="button"
                    class="cs-trigger sm"
                    @click.stop="toggleDropdown('rawLang')"
                  >
                    <span class="lang-tag" :class="langTagClass(rawLangLabel)">
                      {{ langTagShort(rawLangLabel) }}
                    </span>
                    <span class="cs-caret"><svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6" /></svg></span>
                  </button>
                  <div v-if="openDropdown === 'rawLang'" class="cs-menu">
                    <div
                      v-for="l in RAW_LANG_OPTIONS"
                      :key="l"
                      class="cs-item"
                      :class="{ active: activeRequest.rawLang === l }"
                      @click.stop="pickRawLang(l)"
                    >
                      <span class="lang-tag" :class="langTagClass(l)">{{ langTagShort(l) }}</span>
                      <span>{{ l }}</span>
                    </div>
                  </div>
                </div>
                <div class="body-actions">
                  <button
                    class="btn ghost sm body-action-btn"
                    title="JSON 美化"
                    :disabled="!canPrettify"
                    @click="prettifyBody"
                  >
                    Beautify
                  </button>
                  <button
                    v-if="bodyHasJsonFolds"
                    class="btn ghost sm body-action-btn"
                    title="展开全部 JSON 节点"
                    @click="expandAllBodyJson"
                  >
                    展开全部
                  </button>
                  <button
                    class="btn ghost sm body-action-btn"
                    title="清空"
                    @click="clearBody"
                  >
                    清空
                  </button>
                </div>
              </div>

              <div
                v-if="activeRequest.bodyType === 'raw'"
                class="code-editor body-editor"
                :class="{ 'is-folded': bodyHasJsonFolds }"
              >
                <div class="line-numbers" :class="{ foldable: bodyJsonFoldView.valid }">
                  <span v-for="line in bodyLineNumbers" :key="line.sourceLine + (line.foldPath ?? '')">
                    <button
                      v-if="line.foldPath"
                      type="button"
                      class="json-fold-btn"
                      :title="line.folded ? '展开节点' : '收起节点'"
                      @click.stop="toggleBodyJsonFold(line.foldPath)"
                    >{{ line.folded ? "▸" : "▾" }}</button>
                    <i v-else-if="bodyJsonFoldView.valid" class="json-fold-spacer"></i>
                    {{ line.sourceLine }}
                  </span>
                </div>
                <div class="body-edit">
                  <pre
                    ref="bodyHighlightEl"
                    class="code body-highlight"
                    :class="{ interactive: bodyHasJsonFolds }"
                    :aria-hidden="!bodyHasJsonFolds"
                    v-html="bodyHighlighted"
                    @click="onJsonFoldClick($event, toggleBodyJsonFold)"
                  ></pre>
                  <textarea
                    v-show="!bodyHasJsonFolds"
                    ref="bodyTextareaEl"
                    v-model="activeRequest.body"
                    class="code body-textarea"
                    spellcheck="false"
                    :placeholder="'输入请求体（' + activeRequest.rawLang + '）'"
                    @scroll="syncBodyScroll"
                    @mousedown="startBodySelectionDrag"
                    @paste="onBodyPaste"
                  ></textarea>
                </div>
              </div>
              <div v-else-if="activeRequest.bodyType === 'none'" class="placeholder">
                该请求不包含请求体
              </div>
              <div
                v-else-if="
                  activeRequest.bodyType === 'form-data' ||
                  activeRequest.bodyType === 'x-www-form-urlencoded'
                "
                class="body-table-wrap"
              >
                <table class="kv-table body-kv-table">
                  <thead>
                    <tr>
                      <th class="check"></th>
                      <th v-if="activeRequest.bodyType === 'form-data'" class="body-type-col">Type</th>
                      <th>Key</th>
                      <th>Value</th>
                      <th>Description</th>
                      <th></th>
                    </tr>
                  </thead>
                  <tbody>
                    <tr v-for="(row, i) in activeRequest.bodyRows" :key="i">
                      <td class="check"><input v-model="row.enabled" type="checkbox" /></td>
                      <td v-if="activeRequest.bodyType === 'form-data'" class="body-type-col">
                        <div class="field-type-switch">
                          <button
                            type="button"
                            :class="{ active: row.fieldType === 'text' }"
                            @click="row.fieldType = 'text'"
                          >
                            Text
                          </button>
                          <button
                            type="button"
                            :class="{ active: row.fieldType === 'file' }"
                            @click="row.fieldType = 'file'"
                          >
                            File
                          </button>
                        </div>
                      </td>
                      <td>
                        <input
                          v-model="row.key"
                          type="text"
                          placeholder="Key"
                          @input="ensureTrailingBodyRow(activeRequest.bodyRows)"
                        />
                      </td>
                      <td>
                        <div v-if="activeRequest.bodyType === 'form-data' && row.fieldType === 'file'" class="file-cell">
                          <button class="btn ghost sm" @click="pickBodyRowFile(row)">选择文件</button>
                          <span class="file-path" :title="row.filePath">
                            {{ row.fileName || row.filePath || "未选择文件" }}
                          </span>
                        </div>
                        <input
                          v-else
                          v-model="row.value"
                          type="text"
                          placeholder="Value"
                          @input="ensureTrailingBodyRow(activeRequest.bodyRows)"
                        />
                      </td>
                      <td>
                        <input v-model="row.description" type="text" placeholder="Description" />
                      </td>
                      <td><button class="icon-btn xs" @click="removeBodyRow(activeRequest.bodyRows, i)">×</button></td>
                    </tr>
                  </tbody>
                </table>
                <div
                  v-if="activeRequest.bodyType === 'x-www-form-urlencoded'"
                  class="hint-line"
                >
                  将使用 application/x-www-form-urlencoded 编码发送以上已启用键值对
                </div>
                <div
                  v-else
                  class="hint-line"
                >
                  将使用 multipart/form-data 发送以上已启用字段，Content-Type 会自动带 boundary
                </div>
              </div>
              <div v-else-if="activeRequest.bodyType === 'binary'" class="binary-picker">
                <button class="btn ghost" @click="pickBinaryFile(activeRequest)">
                  选择 binary 文件
                </button>
                <span class="file-path" :title="activeRequest.binaryPath">
                  {{ activeRequest.binaryPath || "未选择文件" }}
                </span>
              </div>
            </div>

            <div v-else-if="requestTab === 'auth'" class="param-content active">
              <div class="placeholder">在此配置鉴权方式：Bearer Token、Basic Auth、API Key …</div>
            </div>
            <div v-else-if="requestTab === 'tests'" class="param-content active tests-panel">
              <div class="tests-toolbar">
                <div class="tests-toolbar-label">测试脚本（在响应回来后执行，JS 语法）</div>
                <div class="tests-snippets">
                  <button class="chip-btn" @click="insertTestSnippet('status')" title="断言状态码为 200">Status 2xx</button>
                  <button class="chip-btn" @click="insertTestSnippet('time')" title="断言响应耗时 < 500ms">Time &lt; 500ms</button>
                  <button class="chip-btn" @click="insertTestSnippet('json')" title="解析响应 JSON 并断言字段">JSON body</button>
                  <button class="chip-btn" @click="insertTestSnippet('header')" title="断言 Content-Type 包含 json">Header 包含</button>
                  <button
                    class="chip-btn"
                    @click="formatTestScript"
                    v-if="activeRequest.tests"
                    title="格式化 JS 代码（缩进 & 换行）"
                  >格式化</button>
                  <button class="chip-btn ghost" @click="activeRequest.tests = ''" v-if="activeRequest.tests">清空</button>
                </div>
              </div>
              <div class="tests-editor-wrap">
                <pre
                  ref="testsHighlightEl"
                  class="tests-highlight"
                  aria-hidden="true"
                  v-html="highlightedTests"
                ></pre>
                <textarea
                  ref="testsEditorEl"
                  class="tests-editor"
                  v-model="activeRequest.tests"
                  spellcheck="false"
                  @scroll="syncTestsScroll"
                  @input="syncTestsScroll"
                  placeholder="pm.test('status is 200', () =&gt; {&#10;  pm.expect(pm.response.status).to.equal(200);&#10;});"
                ></textarea>
              </div>
              <div class="tests-hint">
                可用：<code>pm.response.status / json() / text / headers.get(name) / elapsedMs</code>；
                <code>pm.test(name, fn)</code>；
                <code>pm.expect(x).to.equal / eql / include / have.status / be.oneOf / match</code>
              </div>
            </div>
            </template>

            <template v-else>
              <div class="param-tabs">
                <div
                  class="param-tab"
                  :class="{ active: requestTab === 'message' }"
                  @click="requestTab = 'message'"
                >
                  Message
                </div>
                <div
                  class="param-tab"
                  :class="{ active: requestTab === 'metadata' }"
                  @click="requestTab = 'metadata'"
                >
                  Metadata
                  <span v-if="grpcMetaCount" class="badge">{{ grpcMetaCount }}</span>
                </div>
                <div
                  class="param-tab"
                  :class="{ active: requestTab === 'auth' }"
                  @click="requestTab = 'auth'"
                >
                  Proto
                </div>
              </div>

              <div v-if="requestTab === 'message'" class="param-content active">
                <div class="grpc-call-row">
                  <template v-if="activeProtoServices.length">
                    <div
                      class="custom-select grpc-select service-select"
                      :class="{ open: openDropdown === 'grpcService' }"
                    >
                      <button
                        type="button"
                        class="cs-trigger"
                        @click.stop="toggleDropdown('grpcService')"
                      >
                        <span class="cs-value">{{ activeRequest.grpc.service || '选择 service' }}</span>
                        <span class="cs-caret"><svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6" /></svg></span>
                      </button>
                      <div v-if="openDropdown === 'grpcService'" class="cs-menu grpc-menu">
                        <div class="grpc-menu-search">
                          <input
                            ref="grpcServiceSearchEl"
                            v-model="grpcServiceSearch"
                            type="text"
                            placeholder="搜索 service / method…"
                            @click.stop
                            @keydown.esc.prevent="closeDropdown()"
                          />
                        </div>
                        <div
                          v-for="s in filteredGrpcServices"
                          :key="s.name"
                          class="cs-item"
                          :class="{ active: activeRequest.grpc.service === s.name }"
                          @click.stop="pickGrpcService(s.name)"
                        >
                          <span class="grpc-tag sm">SVC</span>
                          <span class="cs-value">{{ s.name }}</span>
                          <span class="muted">{{ s.methods.length }} 个方法</span>
                        </div>
                        <div v-if="!filteredGrpcServices.length" class="cs-item disabled">
                          <span class="muted">无匹配 service</span>
                        </div>
                      </div>
                    </div>
                    <span class="grpc-sep">/</span>
                    <div
                      class="custom-select grpc-select method-select-grpc"
                      :class="{ open: openDropdown === 'grpcMethod' }"
                    >
                      <button
                        type="button"
                        class="cs-trigger"
                        :disabled="!activeProtoMethods.length"
                        @click.stop="toggleDropdown('grpcMethod')"
                      >
                        <span class="cs-value">{{ activeRequest.grpc.method || '选择 method' }}</span>
                        <span class="cs-caret"><svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6" /></svg></span>
                      </button>
                      <div v-if="openDropdown === 'grpcMethod'" class="cs-menu grpc-menu">
                        <div class="grpc-menu-search">
                          <input
                            ref="grpcMethodSearchEl"
                            v-model="grpcMethodSearch"
                            type="text"
                            placeholder="搜索 method / 类型名…"
                            @click.stop
                            @keydown.esc.prevent="closeDropdown()"
                          />
                        </div>
                        <div
                          v-for="m in filteredGrpcMethods"
                          :key="m.name"
                          class="cs-item"
                          :class="{ active: activeRequest.grpc.method === m.name }"
                          @click.stop="pickGrpcMethod(m.name)"
                        >
                          <span class="cs-value">{{ m.name }}</span>
                          <span
                            v-if="m.client_streaming || m.server_streaming"
                            class="grpc-stream-tag"
                            :title="streamingLabel(m)"
                          >{{ streamingShort(m) }}</span>
                          <span class="muted">{{ shortTypeName(m.input_type) }} → {{ shortTypeName(m.output_type) }}</span>
                        </div>
                        <div v-if="!filteredGrpcMethods.length" class="cs-item disabled">
                          <span class="muted">无匹配 method</span>
                        </div>
                      </div>
                    </div>
                  </template>
                  <template v-else>
                    <input
                      v-model="activeRequest.grpc.service"
                      type="text"
                      class="grpc-input"
                      placeholder="Service，例如 user.UserService"
                    />
                    <span class="grpc-sep">/</span>
                    <input
                      v-model="activeRequest.grpc.method"
                      type="text"
                      class="grpc-input"
                      placeholder="Method，例如 GetUser"
                    />
                  </template>
                  <button
                    class="btn ghost sm"
                    title="格式化 JSON 消息"
                    :disabled="!activeRequest.grpc.message"
                    @click="prettifyGrpcMessage"
                  >
                    Beautify
                  </button>
                  <button
                    class="btn ghost sm"
                    title="根据当前 method 的 input 描述符重新生成 JSON 模板（会覆盖现有内容）"
                    :disabled="!activeProtoMethods.length"
                    @click="resetGrpcMessageToTemplate"
                  >
                    模板
                  </button>
                </div>
                <div class="grpc-proto-status" :class="grpcParseStatusClass">
                  <span class="status-dot"></span>
                  <span class="grpc-proto-status-text">{{ grpcParseStatusText }}</span>
                  <button
                    v-if="activeRequest.grpc.protoPath"
                    class="btn ghost xs grpc-reparse-btn"
                    title="重新解析 .proto"
                    @click="reparseActiveProto"
                  >
                    刷新
                  </button>
                </div>
                <div class="code-editor body-editor grpc-message-editor">
                  <div class="line-numbers">
                    <span v-for="n in grpcMessageLineNumbers" :key="n">{{ n }}</span>
                  </div>
                  <div class="body-edit">
                    <pre
                      class="code body-highlight"
                      aria-hidden="true"
                      v-html="grpcMessageHighlighted"
                    ></pre>
                    <textarea
                      v-model="activeRequest.grpc.message"
                      class="code body-textarea"
                      spellcheck="false"
                      placeholder="请求消息（JSON 格式，由 .proto 中的请求 message 类型决定字段）"
                    ></textarea>
                  </div>
                </div>
              </div>

              <div v-else-if="requestTab === 'metadata'" class="param-content active">
                <table class="kv-table">
                  <thead>
                    <tr>
                      <th class="check"></th>
                      <th>Key</th>
                      <th>Value</th>
                      <th>Description</th>
                      <th></th>
                    </tr>
                  </thead>
                  <tbody>
                    <tr v-for="(row, i) in activeRequest.grpc.metadata" :key="i">
                      <td class="check"><input v-model="row.enabled" type="checkbox" /></td>
                      <td>
                        <input
                          v-model="row.key"
                          type="text"
                          placeholder="metadata-key（小写）"
                          @input="ensureTrailingRow(activeRequest.grpc.metadata)"
                        />
                      </td>
                      <td>
                        <input
                          v-model="row.value"
                          type="text"
                          placeholder="Value"
                          @input="ensureTrailingRow(activeRequest.grpc.metadata)"
                        />
                      </td>
                      <td>
                        <input v-model="row.description" type="text" placeholder="Description" />
                      </td>
                      <td><button class="icon-btn xs" @click="removeRow(activeRequest.grpc.metadata, i)">×</button></td>
                    </tr>
                  </tbody>
                </table>
              </div>

              <div v-else-if="requestTab === 'auth'" class="param-content active">
                <div class="grpc-proto">
                  <label class="form-label">.proto 文件路径</label>
                  <div class="grpc-file-row">
                    <input
                      v-model="activeRequest.grpc.protoPath"
                      type="text"
                      class="modal-input"
                      placeholder="/绝对/路径/到/your.proto"
                    />
                    <button class="btn ghost sm" @click="pickProtoFile">浏览…</button>
                  </div>
                  <label class="form-label">Import 目录（每行一个，可选）</label>
                  <textarea
                    v-model="activeRequest.grpc.importPaths"
                    class="modal-input grpc-import-textarea"
                    spellcheck="false"
                    placeholder="/绝对/路径/到/proto-root&#10;/another/proto-root"
                  ></textarea>
                  <label class="form-label">Authority（可选，TLS SNI / :authority）</label>
                  <input
                    v-model="activeRequest.grpc.authority"
                    type="text"
                    class="modal-input"
                    placeholder="例如 my-service.example.com"
                  />
                  <p class="grpc-hint">
                    需在「Message」页填写 <code>Service</code>（完整带 package 名）和
                    <code>Method</code>。当前仅支持 <strong>一元（unary）</strong>调用。
                  </p>
                </div>
              </div>

              <div v-else class="param-content active">
                <div class="placeholder">请切换至 Message / Metadata / Proto 页签。</div>
              </div>
            </template>
          </div>

          <div
            class="splitter"
            title="拖动调整请求/响应高度"
            @mousedown="startRowsDrag"
          >
            <span class="grip"></span>
          </div>

          <div class="response-pane">
            <div class="response-meta">
              <div class="meta-left">
                <span v-if="responseStatus !== null" class="status" :class="statusClass">
                  {{ responseStatus }} {{ statusText(responseStatus) }}
                </span>
                <span v-else class="status idle">未发送</span>
                <span v-if="responseTimeMs !== null" class="meta-item">Time <b>{{ responseTimeMs }} ms</b></span>
                <span v-if="responseSize !== null" class="meta-item">Size <b>{{ sizeLabel }}</b></span>
                <span v-if="errorMessage" class="status err">{{ errorMessage }}</span>
              </div>
              <div class="meta-right">
                <button class="icon-btn xs" title="复制" @click="copyResponse">⧉</button>
              </div>
            </div>

            <div class="param-tabs sm">
              <div
                class="param-tab"
                :class="{ active: responseSubTab === 'body' }"
                @click="responseSubTab = 'body'"
              >
                Body
              </div>
              <div
                class="param-tab"
                :class="{ active: responseSubTab === 'headers' }"
                @click="responseSubTab = 'headers'"
              >
                Headers
                <span v-if="responseHeaders.length" class="badge">{{ responseHeaders.length }}</span>
              </div>
              <div
                class="param-tab"
                :class="{ active: responseSubTab === 'cookies' }"
                @click="responseSubTab = 'cookies'"
              >
                Cookies
              </div>
              <div
                class="param-tab"
                :class="{ active: responseSubTab === 'tests' }"
                @click="responseSubTab = 'tests'"
              >
                Test Results
                <span v-if="responseTests.length" class="badge" :class="{ err: testsFailedCount > 0 }">
                  {{ testsPassedCount }}/{{ responseTests.length }}
                </span>
              </div>
            </div>

            <div v-if="responseSubTab === 'body'" class="response-body">
              <div class="resp-toolbar">
                <div class="seg">
                  <button
                    class="seg-btn"
                    :class="{ active: responseFormat === 'pretty' }"
                    @click="responseFormat = 'pretty'"
                  >
                    Pretty
                  </button>
                  <button
                    class="seg-btn"
                    :class="{ active: responseFormat === 'raw' }"
                    @click="responseFormat = 'raw'"
                  >
                    Raw
                  </button>
                  <button
                    class="seg-btn"
                    :class="{ active: responseFormat === 'preview' }"
                    @click="responseFormat = 'preview'"
                  >
                    Preview
                  </button>
                </div>
                <div
                  class="custom-select sm"
                  :class="{ open: openDropdown === 'respLang' }"
                >
                  <button
                    type="button"
                    class="cs-trigger sm"
                    @click.stop="toggleDropdown('respLang')"
                  >
                    <span class="lang-tag" :class="langTagClass(responseLang)">
                      {{ langTagShort(responseLang) }}
                    </span>
                    <span class="cs-caret"><svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6" /></svg></span>
                  </button>
                  <div v-if="openDropdown === 'respLang'" class="cs-menu align-end">
                    <div
                      v-for="l in RESPONSE_LANG_OPTIONS"
                      :key="l"
                      class="cs-item"
                      :class="{ active: responseLang === l }"
                      @click.stop="pickResponseLang(l)"
                    >
                      <span class="lang-tag" :class="langTagClass(l)">{{ langTagShort(l) }}</span>
                      <span>{{ l }}</span>
                    </div>
                  </div>
                </div>
                <button
                  v-if="responseJsonCollapsed.length"
                  class="btn ghost sm"
                  title="展开全部 JSON 节点"
                  @click="expandAllResponseJson"
                >
                  展开全部
                </button>
              </div>
              <div v-if="responseFormat !== 'preview'" class="code-editor">
                <div
                  ref="responseGutterEl"
                  class="line-numbers"
                  :class="{ foldable: responseJsonFoldView.valid }"
                  @wheel.prevent="onResponseGutterWheel"
                >
                  <span v-for="line in responseLineNumbers" :key="line.sourceLine + (line.foldPath ?? '')">
                    <button
                      v-if="line.foldPath"
                      type="button"
                      class="json-fold-btn"
                      :title="line.folded ? '展开节点' : '收起节点'"
                      @click.stop="toggleResponseJsonFold(line.foldPath)"
                    >{{ line.folded ? "▸" : "▾" }}</button>
                    <i v-else-if="responseJsonFoldView.valid" class="json-fold-spacer"></i>
                    {{ line.sourceLine }}
                  </span>
                </div>
                <pre
                  v-if="responseBody"
                  ref="responseCodeEl"
                  class="code"
                  :class="{ interactive: responseJsonFoldView.valid }"
                  v-html="highlightedResponse"
                  @click="onJsonFoldClick($event, toggleResponseJsonFold)"
                  @scroll="syncResponseGutterScroll"
                ></pre>
                <pre v-else class="code muted">// 等待请求…</pre>
              </div>
              <div v-else class="code-editor preview">
                <iframe class="preview-frame" :srcdoc="responsePreviewSrc" sandbox=""></iframe>
              </div>
            </div>

            <div v-else-if="responseSubTab === 'headers'" class="response-body">
              <table class="kv-table">
                <thead>
                  <tr>
                    <th>Key</th>
                    <th>Value</th>
                  </tr>
                </thead>
                <tbody>
                  <tr v-for="(h, i) in responseHeaders" :key="i">
                    <td><input :value="h.key" type="text" readonly /></td>
                    <td><input :value="h.value" type="text" readonly /></td>
                  </tr>
                  <tr v-if="!responseHeaders.length">
                    <td colspan="2"><div class="placeholder">尚未发送请求</div></td>
                  </tr>
                </tbody>
              </table>
            </div>

            <div v-else-if="responseSubTab === 'tests'" class="response-body tests-results">
              <div v-if="!responseTests.length" class="placeholder">
                当前请求没有测试脚本，或响应尚未回来。可在请求面板的 <b>Tests</b> Tab 编辑脚本。
              </div>
              <template v-else>
                <div class="tests-summary">
                  <span class="chip" :class="{ ok: testsFailedCount === 0, err: testsFailedCount > 0 }">
                    {{ testsPassedCount }} passed
                  </span>
                  <span class="chip err" v-if="testsFailedCount > 0">
                    {{ testsFailedCount }} failed
                  </span>
                  <span class="chip">total {{ responseTests.length }}</span>
                </div>
                <ul class="tests-list">
                  <li
                    v-for="(t, i) in responseTests"
                    :key="i"
                    class="tests-item"
                    :class="{ ok: t.passed, err: !t.passed }"
                  >
                    <div class="tests-item-head">
                      <span class="tests-status">
                        <svg v-if="t.passed" viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>
                        <svg v-else viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>
                      </span>
                      <span class="tests-name">{{ t.name }}</span>
                      <span class="tests-duration">{{ t.durationMs }}ms</span>
                    </div>
                    <pre v-if="!t.passed && t.error" class="tests-error">{{ t.error }}</pre>
                  </li>
                </ul>
              </template>
            </div>

            <div v-else class="placeholder">该面板暂未实现</div>
          </div>
        </template>

        <div v-else class="empty-workspace">
          <div class="empty-card">
            <h3>暂无打开的请求</h3>
            <p>从左侧集合中选择一条请求，或点击下面按钮新建请求开始测试。</p>
            <div
              class="new-split inline"
              :class="{ open: openDropdown === 'newEmpty' }"
            >
              <button class="btn primary new-main" @click="newTab('http')">+ 新建 HTTP 请求</button>
              <button
                class="btn primary new-caret"
                title="选择请求类型"
                @click.stop="toggleDropdown('newEmpty')"
              >
                <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6" /></svg>
              </button>
              <div v-if="openDropdown === 'newEmpty'" class="cs-menu align-end new-menu">
                <div class="cs-item" @click.stop="newTab('http')">
                  <span class="method get">GET</span>
                  <span>HTTP 请求</span>
                </div>
                <div class="cs-item" @click.stop="newTab('grpc')">
                  <span class="method grpc">gRPC</span>
                  <span>gRPC 请求</span>
                </div>
              </div>
            </div>
          </div>
        </div>
        </template>
      </section>
    </main>

    <div v-if="showImportModal" class="modal-mask" @click.self="closeImportModal">
      <div class="modal-card import-card">
        <div class="modal-head">
          <div class="modal-title">
            <span class="modal-title-icon">
              <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <path d="M5 5 15 15" />
                <path d="M15 7v8h-8" />
                <path d="M4 20h16" />
              </svg>
            </span>
            <strong>导入请求</strong>
          </div>
          <button class="modal-close" title="关闭" @click="closeImportModal">×</button>
        </div>

        <div class="modal-tabs">
          <div
            class="m-tab"
            :class="{ active: importSource === 'curl' }"
            @click="importSource = 'curl'"
          >
            cURL
          </div>
          <div class="m-tab disabled" title="后续支持">OpenAPI / Swagger</div>
          <div class="m-tab disabled" title="后续支持">HAR</div>
          <div class="m-tab disabled" title="后续支持">Postman 集合</div>
        </div>

        <div class="modal-body">
          <label class="form-label">粘贴 cURL 命令</label>
          <div class="curl-input-wrap">
            <textarea
              v-model="importCurlText"
              class="curl-input"
              spellcheck="false"
              placeholder="例：curl -X POST &quot;https://api.example.com/v1/users&quot; \
  -H &quot;Content-Type: application/json&quot; \
  -H &quot;Authorization: Bearer xxx&quot; \
  -d '{&quot;name&quot;:&quot;Alice&quot;,&quot;age&quot;:28}'"
            ></textarea>
            <div
              class="curl-status"
              :class="{
                valid: importParsed && !importError,
                invalid: !!importError,
                muted: !importParsed && !importError,
              }"
            >
              <span class="status-dot"></span>
              <span class="status-text">
                <template v-if="importError">{{ importError }}</template>
                <template v-else-if="importParsed">合法的 cURL · 已识别 1 个请求</template>
                <template v-else>等待输入 / 粘贴 cURL 命令</template>
              </span>
            </div>
          </div>

          <div v-if="importParsed" class="parse-preview">
            <div class="pp-title">解析结果预览</div>
            <div class="pp-row">
              <span class="pp-label">方法</span>
              <span class="method" :class="methodKey(importParsed.method)">
                {{ methodTagText(importParsed.method) }}
              </span>
            </div>
            <div class="pp-row">
              <span class="pp-label">URL</span>
              <span class="pp-url" :title="importParsed.url">{{ importParsed.url }}</span>
            </div>
            <div class="pp-row">
              <span class="pp-label">Headers</span>
              <template v-if="importParsedHeaderKeys.length">
                <span v-for="k in importParsedHeaderKeys" :key="k" class="pp-tag">{{ k }}</span>
              </template>
              <span v-else class="pp-empty">无</span>
            </div>
            <div class="pp-row">
              <span class="pp-label">Body</span>
              <span
                v-if="importParsed.body"
                class="pp-tag"
                :class="importParsedBodyTagClass"
              >
                {{ importParsedBodyTagText }} · {{ importParsedBodySize }}
              </span>
              <span v-else class="pp-empty">无</span>
            </div>
          </div>

          <label class="form-label modal-section-label">保存到集合</label>
          <div class="form-row">
            <div
              class="custom-select col-cdrop"
              :class="{ open: openDropdown === 'importFolder' }"
            >
              <button
                type="button"
                class="cs-trigger col-trigger"
                @click.stop="toggleDropdown('importFolder')"
              >
                <span v-if="importTargetFolderId === '__new__'" class="dd-icon">＋</span>
                <svg v-else class="folder-icon" viewBox="0 0 24 24" aria-hidden="true">
                  <path d="M3 7a2 2 0 0 1 2-2h4.17a2 2 0 0 1 1.42.59l1.41 1.41A2 2 0 0 0 13.41 8H19a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
                </svg>
                <span class="cs-value">{{ importTargetFolderName }}</span>
                <span class="cs-caret">
                  <svg viewBox="0 0 24 24" width="10" height="10" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
                    <path d="m6 9 6 6 6-6" />
                  </svg>
                </span>
              </button>
              <div v-if="openDropdown === 'importFolder'" class="cs-menu col-menu">
                <div
                  v-for="f in folders"
                  :key="f.id"
                  class="cs-item"
                  :class="{ active: importTargetFolderId === f.id }"
                  @click.stop="pickImportFolder(f.id)"
                >
                  <svg class="folder-icon" viewBox="0 0 24 24" aria-hidden="true">
                    <path d="M3 7a2 2 0 0 1 2-2h4.17a2 2 0 0 1 1.42.59l1.41 1.41A2 2 0 0 0 13.41 8H19a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
                  </svg>
                  <span>{{ f.name }}</span>
                </div>
                <div v-if="folders.length" class="cs-divider"></div>
                <div
                  class="cs-item action"
                  :class="{ active: importTargetFolderId === '__new__' }"
                  @click.stop="pickImportFolder('__new__')"
                >
                  <span class="dd-icon">
                    <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
                      <path d="M12 5v14M5 12h14" />
                    </svg>
                  </span>
                  <span>新建集合…</span>
                </div>
              </div>
            </div>
            <input
              v-model="importRequestName"
              type="text"
              class="modal-input"
              placeholder="请求名称（默认：方法 + 路径）"
            />
          </div>
          <input
            v-if="importTargetFolderId === '__new__'"
            v-model="importNewFolderName"
            type="text"
            class="modal-input new-folder-input"
            placeholder="新集合名称"
          />
        </div>

        <div class="modal-foot">
          <button class="btn ghost" @click="closeImportModal">取消</button>
          <button
            class="btn primary"
            :disabled="!importParsed"
            @click="confirmImport"
          >
            导入并打开
          </button>
        </div>
      </div>
    </div>

    <div v-if="promptState.visible" class="modal-mask" @click.self="cancelPrompt">
      <div class="modal-card prompt-card">
        <div class="modal-head">
          <div class="modal-title">
            <span class="modal-title-icon" aria-hidden="true">
              <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <path d="M12 20h9" />
                <path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4L16.5 3.5z" />
              </svg>
            </span>
            <strong>{{ promptState.title }}</strong>
          </div>
          <button class="modal-close" title="关闭" @click="cancelPrompt" aria-label="关闭">
            <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
              <path d="M18 6 6 18" />
              <path d="m6 6 12 12" />
            </svg>
          </button>
        </div>
        <div class="modal-body">
          <label v-if="promptState.label" class="form-label">{{ promptState.label }}</label>
          <input
            ref="promptInputEl"
            v-model="promptState.value"
            type="text"
            class="modal-input"
            :placeholder="promptState.placeholder"
            @keydown.enter.prevent="confirmPrompt"
            @keydown.esc.prevent="cancelPrompt"
          />
        </div>
        <div class="modal-foot">
          <button class="btn ghost" @click="cancelPrompt">{{ promptState.cancelText }}</button>
          <button
            class="btn primary"
            :disabled="!promptState.value.trim()"
            @click="confirmPrompt"
          >
            {{ promptState.confirmText }}
          </button>
        </div>
      </div>
    </div>

    <div v-if="confirmState.visible" class="modal-mask" @click.self="answerConfirm(false)">
      <div class="modal-card confirm-card" :class="{ danger: confirmState.danger }">
        <button class="confirm-close" title="关闭" @click="answerConfirm(false)">×</button>
        <div class="confirm-body">
          <div class="confirm-icon" aria-hidden="true">
            <svg v-if="confirmState.danger" viewBox="0 0 24 24" width="26" height="26" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
              <path d="M3 6h18" />
              <path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
              <path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
              <path d="M10 11v6M14 11v6" />
            </svg>
            <svg v-else viewBox="0 0 24 24" width="26" height="26" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
              <circle cx="12" cy="12" r="9" />
              <path d="M12 8v4M12 16h.01" />
            </svg>
          </div>
          <div class="confirm-text">
            <div class="confirm-title">{{ confirmState.title }}</div>
            <p class="confirm-message">{{ confirmState.message }}</p>
          </div>
        </div>
        <div class="confirm-foot">
          <button class="btn ghost" @click="answerConfirm(false)">{{ confirmState.cancelText }}</button>
          <button
            class="btn"
            :class="confirmState.danger ? 'danger' : 'primary'"
            @click="answerConfirm(true)"
          >
            {{ confirmState.confirmText }}
          </button>
        </div>
      </div>
    </div>

    <footer class="statusbar" data-tauri-drag-region>
      <span class="status-dot">●</span>
      <span>已连接</span>
      <span class="dot-sep">·</span>
      <span>本地存储：{{ storagePathLabel }}</span>
      <span class="dot-sep">·</span>
      <span>v0.1.0</span>
      <div class="status-right">
        <span>UTF-8</span>
        <span class="dot-sep">·</span>
        <span>{{ responseLang }}</span>
      </div>
    </footer>

    <div class="window-drag-rail right" data-tauri-drag-region aria-hidden="true"></div>
  </div>
</template>

<style>
:root,
:root[data-theme="dark"] {
  --bg: #1e1f24;
  --bg-2: #252731;
  --bg-3: #2c2e3a;
  --bg-hover: #2f3242;
  --panel: #20222b;
  --border: #353846;
  --border-2: #2a2c38;
  --text: #e6e8ef;
  --text-2: #a8acbb;
  --text-3: #6c7088;
  --primary: #ff7a3d;
  --primary-hover: #ff8d57;
  --primary-press: #ef6b2c;
  --accent: #5b8cff;
  --green: #2ec27e;
  --yellow: #f5b942;
  --red: #f06363;
  --purple: #b07cff;
  --blue: #4aa9ff;
  --radius: 6px;
  --radius-sm: 4px;
  --shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
  --mono: ui-monospace, SFMono-Regular, "JetBrains Mono", Menlo, Consolas, monospace;
  --sans: -apple-system, "SF Pro Text", "PingFang SC", "Microsoft YaHei", system-ui, sans-serif;

  --code-bg: #161821;
  --code-gutter: #14151c;
  --rail-bg: #16171e;
  --statusbar-bg: #14151c;
  --titlebar-bg: linear-gradient(180deg, #25283a 0%, #1d1f29 100%);
  --modal-mask: rgba(8, 9, 14, 0.6);
  --row-hover: rgba(255, 255, 255, 0.02);
  --kbd-bg: var(--bg);

  --code-key: #79b8ff;
  --code-str: #ff8b5b;
  --code-num: #66d4a8;
  --code-bool: #c586c0;
  --code-punc: #d4d4d4;
}

:root[data-theme="light"] {
  --bg: #f6f7fb;
  --bg-2: #ffffff;
  --bg-3: #eef0f5;
  --bg-hover: #e6e9f1;
  --panel: #ffffff;
  --border: #d8dbe3;
  --border-2: #e7e9ef;
  --text: #1f2128;
  --text-2: #5b6173;
  --text-3: #8a91a3;
  --primary: #ff6a2b;
  --primary-hover: #ff7e47;
  --primary-press: #e35718;
  --accent: #3a6ddc;
  --green: #1f9e63;
  --yellow: #cf9419;
  --red: #d23b3b;
  --purple: #8b5cf6;
  --blue: #2f7dd8;
  --shadow: 0 8px 24px rgba(20, 30, 60, 0.10);

  --code-bg: #fbfbfd;
  --code-gutter: #f1f3f8;
  --rail-bg: #f0f2f7;
  --statusbar-bg: #f0f2f7;
  --titlebar-bg: linear-gradient(180deg, #ffffff 0%, #eef0f7 100%);
  --modal-mask: rgba(20, 30, 60, 0.32);
  --row-hover: rgba(15, 30, 80, 0.04);
  --kbd-bg: #ffffff;

  --code-key: #0066cc;
  --code-str: #c4421a;
  --code-num: #007a4d;
  --code-bool: #6f42c1;
  --code-punc: #4d5566;
}

* {
  box-sizing: border-box;
}

html,
body,
#app {
  margin: 0;
  padding: 0;
  height: 100%;
  background: var(--bg);
  color: var(--text);
  font-family: var(--sans);
  font-size: 13px;
  -webkit-font-smoothing: antialiased;
  user-select: none;
}

button {
  font-family: inherit;
}

.app-root {
  display: flex;
  flex-direction: column;
  height: 100%;
  position: relative;
}

.window-drag-rail {
  position: absolute;
  top: 40px;
  bottom: 24px;
  width: 6px;
  z-index: 50;
  background: transparent;
  pointer-events: auto;
  cursor: default;
}
.window-drag-rail.right {
  right: 0;
}

/* === 顶部工具栏 === */
.titlebar {
  height: 40px;
  display: flex;
  align-items: center;
  justify-content: center;
  position: relative;
  padding: 0 12px;
  background: var(--titlebar-bg);
  border-bottom: 1px solid var(--border-2);
  -webkit-app-region: drag;
}
.app-title-button {
  border: none;
  background: transparent;
  color: var(--text-2);
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0.4px;
  padding: 4px 10px;
  border-radius: var(--radius);
  -webkit-app-region: no-drag;
}
.app-title-button:hover {
  background: var(--bg-hover);
  color: var(--text);
}
.app-logo {
  width: 18px;
  height: 18px;
  display: block;
  pointer-events: none;
}
.title-actions {
  position: absolute;
  right: 12px;
  top: 50%;
  transform: translateY(-50%);
  display: flex;
  gap: 6px;
  align-items: center;
  -webkit-app-region: no-drag;
}

/* === 主题切换按钮（太阳/月亮叠加） === */
.theme-toggle {
  position: relative;
  width: 28px;
  height: 28px;
  overflow: hidden;
}
.theme-toggle .icn {
  position: absolute;
  top: 50%;
  left: 50%;
  width: 18px;
  height: 18px;
  margin: -9px 0 0 -9px;
  transition: transform 0.42s cubic-bezier(0.4, 0, 0.2, 1),
    opacity 0.28s ease;
}
.theme-toggle .sun {
  color: #f5b942;
  transform: rotate(0deg) scale(1);
  opacity: 1;
}
.theme-toggle .moon {
  color: #c5cae9;
  transform: rotate(-65deg) scale(0.4);
  opacity: 0;
}
:root[data-theme="dark"] .theme-toggle .sun {
  transform: rotate(65deg) scale(0.4);
  opacity: 0;
}
:root[data-theme="dark"] .theme-toggle .moon {
  color: #d8defd;
  transform: rotate(0deg) scale(1);
  opacity: 1;
}
.theme-toggle:hover .icn {
  filter: drop-shadow(0 0 4px currentColor);
}

/* === 主体布局 === */
.app {
  display: grid;
  grid-template-columns: 64px var(--sidebar-w, 280px) 4px 1fr;
  flex: 1;
  min-height: 0;
}

.v-resizer {
  width: 4px;
  cursor: col-resize;
  background: transparent;
  position: relative;
  z-index: 5;
}
.v-resizer::after {
  content: "";
  position: absolute;
  inset: 0 1px;
  background: var(--border-2);
  transition: background 0.15s;
}
.v-resizer:hover::after,
.v-resizer:active::after {
  background: var(--primary);
}

body.dragging-x,
body.dragging-x * {
  cursor: col-resize !important;
  user-select: none !important;
}
body.dragging-y,
body.dragging-y * {
  cursor: row-resize !important;
  user-select: none !important;
}

/* === 左侧导航条 === */
.nav-rail {
  background: var(--rail-bg);
  border-right: 1px solid var(--border-2);
  display: flex;
  flex-direction: column;
  align-items: stretch;
  padding: 10px 6px;
  gap: 4px;
  -webkit-app-region: drag;
}
.rail-btn {
  background: transparent;
  border: none;
  color: var(--text-3);
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 4px;
  padding: 8px 4px;
  border-radius: var(--radius);
  cursor: pointer;
  font-size: 11px;
  transition: all 0.15s;
  position: relative;
  -webkit-app-region: no-drag;
}
.rail-btn:hover {
  color: var(--text);
  background: var(--bg-2);
}
.rail-btn.active {
  color: var(--primary);
  background: rgba(255, 122, 61, 0.08);
}
.rail-btn.active::before {
  content: "";
  position: absolute;
  left: 0;
  width: 3px;
  height: 24px;
  background: var(--primary);
  border-radius: 0 2px 2px 0;
}
.rail-spacer {
  flex: 1;
  min-height: 24px;
  -webkit-app-region: drag;
}
.env-indicator {
  display: flex;
  flex-direction: column;
  align-items: center;
  font-size: 11px;
  color: var(--text-2);
  padding: 8px 0;
  gap: 4px;
  -webkit-app-region: no-drag;
}
.env-indicator .dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--green);
  box-shadow: 0 0 8px rgba(46, 194, 126, 0.6);
}
.env-indicator .dot.global {
  background: var(--purple);
}
.env-indicator .dot.stg {
  background: var(--yellow);
}
.env-indicator .dot.prd {
  background: var(--red);
}
.env-indicator .dot.custom {
  background: var(--accent);
}

/* === 侧边栏 === */
.sidebar {
  background: var(--panel);
  border-right: 1px solid var(--border-2);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.sidebar .panel {
  display: none;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}
.sidebar .panel.active {
  display: flex;
}
.panel-header {
  padding: 10px;
  display: flex;
  gap: 6px;
  align-items: center;
  border-bottom: 1px solid var(--border-2);
  min-width: 0;
}
.panel-header > .btn,
.panel-header > .panel-act {
  flex-shrink: 0;
}
.panel-header.compact > .search {
  display: none;
}
.search {
  flex: 1 1 0;
  min-width: 0;
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 0 8px;
  display: flex;
  align-items: center;
  gap: 6px;
  height: 30px;
  color: var(--text-3);
  overflow: hidden;
  transition: border-color 0.15s ease, box-shadow 0.15s ease, background 0.15s ease;
}
.search:hover {
  border-color: var(--border-2);
}
.search:focus-within {
  border-color: var(--primary);
  background: var(--bg-2);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--primary) 22%, transparent);
}
.search:focus-within > svg {
  color: var(--primary);
}
.search input {
  background: transparent;
  border: none;
  outline: none;
  color: var(--text);
  flex: 1 1 0;
  min-width: 0;
  font-size: 12px;
  padding: 0;
}
.search input::placeholder {
  color: var(--text-3);
}
.search-clear {
  flex-shrink: 0;
  width: 16px;
  height: 16px;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: var(--bg-3);
  color: var(--text-2);
  font-size: 12px;
  line-height: 1;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  transition: background 0.15s ease, color 0.15s ease;
}
.search-clear:hover {
  background: var(--text-3);
  color: var(--bg);
}

.panel-act {
  flex-shrink: 0;
  width: 30px;
  height: 30px;
  padding: 0;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--bg);
  color: var(--text-2);
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  transition: all 0.15s ease;
}
.panel-act:hover {
  background: var(--bg-2);
  color: var(--text);
  border-color: var(--border-2);
}
.panel-act:active {
  transform: scale(0.95);
}
.panel-act:disabled {
  cursor: not-allowed;
  opacity: 0.4;
}
.panel-act:disabled:hover {
  background: var(--bg);
  color: var(--text-2);
  border-color: var(--border);
}
.panel-act.primary {
  background: var(--primary);
  color: #fff;
  border-color: var(--primary);
  box-shadow: 0 2px 6px color-mix(in srgb, var(--primary) 32%, transparent);
}
.panel-act.primary:hover {
  background: var(--primary-hover);
  color: #fff;
  border-color: var(--primary-hover);
}
.panel-act.danger-hover:not(:disabled):hover {
  background: color-mix(in srgb, var(--red) 14%, transparent);
  color: var(--red);
  border-color: color-mix(in srgb, var(--red) 40%, transparent);
}

/* 通用按钮 */
.btn {
  border: 1px solid transparent;
  border-radius: var(--radius);
  padding: 6px 12px;
  font-size: 12px;
  cursor: pointer;
  transition: all 0.15s;
  background: var(--bg-3);
  color: var(--text);
}
.btn:hover {
  background: var(--bg-hover);
}
.btn:disabled {
  cursor: not-allowed;
  opacity: 0.6;
}
.btn.sm {
  padding: 4px 10px;
  font-size: 11px;
}
.btn.ghost {
  background: transparent;
  color: var(--text-2);
}
.btn.ghost:hover {
  background: var(--bg-2);
  color: var(--text);
}
.btn.primary {
  background: var(--primary);
  color: #fff;
  border-color: var(--primary);
}
.btn.primary:hover {
  background: var(--primary-hover);
}
.btn.primary:active {
  background: var(--primary-press);
}
.btn.danger {
  background: var(--red);
  color: #fff;
  border-color: var(--red);
}
.btn.danger:hover {
  filter: brightness(1.08);
}
.btn.danger:active {
  filter: brightness(0.92);
}
.prompt-card {
  width: 420px;
  max-width: 92vw;
}
.prompt-card .modal-body {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.confirm-message {
  margin: 0;
  font-size: 13px;
  color: var(--text-2);
  line-height: 1.6;
}

/* === 确认对话框 === */
.confirm-card {
  position: relative;
  width: 380px;
  max-width: 92vw;
  padding: 22px 22px 16px;
  border-radius: 14px;
  background: var(--bg-2);
  border: 1px solid var(--border);
  box-shadow: 0 24px 60px rgba(0, 0, 0, 0.32);
  animation: confirm-pop 0.16s ease-out;
}
@keyframes confirm-pop {
  from { opacity: 0; transform: translateY(8px) scale(0.96); }
  to   { opacity: 1; transform: translateY(0) scale(1); }
}
.confirm-close {
  position: absolute;
  top: 10px;
  right: 10px;
  width: 26px;
  height: 26px;
  border: none;
  border-radius: 8px;
  background: transparent;
  color: var(--text-3);
  font-size: 18px;
  line-height: 1;
  cursor: pointer;
  transition: background 0.15s ease, color 0.15s ease;
}
.confirm-close:hover {
  background: var(--bg-3);
  color: var(--text);
}
.confirm-body {
  display: flex;
  gap: 14px;
  align-items: flex-start;
  padding-right: 20px;
}
.confirm-icon {
  flex-shrink: 0;
  width: 44px;
  height: 44px;
  border-radius: 50%;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: color-mix(in srgb, var(--primary) 14%, transparent);
  color: var(--primary);
}
.confirm-card.danger .confirm-icon {
  background: color-mix(in srgb, var(--red) 16%, transparent);
  color: var(--red);
}
.confirm-text {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding-top: 2px;
}
.confirm-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--text);
  letter-spacing: 0.2px;
}
.confirm-foot {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 18px;
}
.confirm-foot .btn {
  min-width: 76px;
  padding: 6px 14px;
  border-radius: 8px;
}
.confirm-card.danger .btn.danger {
  box-shadow: 0 4px 14px color-mix(in srgb, var(--red) 40%, transparent);
}
.icon-btn {
  background: transparent;
  border: none;
  color: var(--text-3);
  width: 24px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-sm);
  cursor: pointer;
  font-size: 14px;
}
.icon-btn:hover {
  background: var(--bg-2);
  color: var(--text);
}
.icon-btn.xs {
  width: 20px;
  height: 20px;
  font-size: 12px;
}

/* === 集合树 === */
.tree {
  padding: 6px 0;
  overflow-y: auto;
  flex: 1;
}
.tree-empty {
  padding: 12px;
  text-align: center;
  color: var(--text-3);
  font-size: 12px;
}
.tree-children {
  display: block;
}
.tree-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  font-size: 12.5px;
  color: var(--text);
  cursor: pointer;
  border-left: 2px solid transparent;
}
.tree-row:hover {
  background: var(--bg-2);
}
.tree-row .caret {
  width: 14px;
  height: 14px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  transform-origin: center;
  transition: transform 0.18s ease, color 0.15s ease;
}
.tree-row .caret svg {
  display: block;
}
.tree-row:hover .caret {
  color: var(--text-2);
}
.tree-row .caret.open {
  transform: rotate(90deg);
  color: var(--primary);
}
.folder-icon {
  width: 16px;
  height: 16px;
  flex-shrink: 0;
  color: var(--text-3);
  fill: none;
  stroke: currentColor;
  stroke-width: 1.6;
  stroke-linecap: round;
  stroke-linejoin: round;
  transition: color 0.15s ease, fill 0.15s ease;
}
.tree-row .folder-icon {
  color: var(--text-2);
}
.tree-row .folder-icon.open {
  color: var(--primary);
  fill: color-mix(in srgb, var(--primary) 18%, transparent);
}
.tree-row .tree-label {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tree-row .tree-actions {
  display: none;
}
.tree-row:hover .tree-actions {
  display: inline-flex;
  gap: 2px;
}
.tree-row.request {
  padding-left: 30px;
  font-size: 12px;
}
.tree-row.request.active {
  background: rgba(255, 122, 61, 0.12);
  border-left-color: var(--primary);
  color: var(--primary);
}
.tree-node.folder.selected > .tree-row {
  background: rgba(255, 122, 61, 0.08);
  border-left-color: var(--primary);
}
.tree-node.folder.selected > .tree-row .folder-icon,
.tree-node.folder.selected > .tree-row .tree-label {
  color: var(--primary);
}
.folder-selected-tag {
  font-size: 10px;
  padding: 1px 6px;
  border-radius: 999px;
  background: rgba(255, 122, 61, 0.18);
  color: var(--primary);
  letter-spacing: 0.5px;
  margin-right: 4px;
}

/* HTTP 方法标签 */
.method {
  font-size: 10px;
  font-weight: 700;
  padding: 1px 4px;
  border-radius: 3px;
  font-family: var(--mono);
  letter-spacing: 0.3px;
  min-width: 32px;
  text-align: center;
  display: inline-block;
}
.method.get {
  color: var(--green);
}
.method.post {
  color: var(--yellow);
}
.method.put {
  color: var(--blue);
}
.method.delete {
  color: var(--red);
}
.method.patch {
  color: var(--purple);
}

/* === 环境列表 === */
.env-list {
  padding: 6px;
  overflow-y: auto;
  flex: 0 0 auto;
  max-height: 35%;
}
.env-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border-radius: var(--radius);
  cursor: pointer;
  margin-bottom: 2px;
  font-size: 12.5px;
}
.env-item:hover {
  background: var(--bg-2);
}
.env-item.active {
  background: rgba(255, 122, 61, 0.1);
  color: var(--primary);
}
.env-item .dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--text-3);
}
.env-item .dot.global {
  background: var(--purple);
}
.env-item .dot.dev {
  background: var(--green);
}
.env-item .dot.stg {
  background: var(--yellow);
}
.env-item .dot.prd {
  background: var(--red);
}
.env-item .dot.custom {
  background: var(--accent);
}
.env-item .env-name {
  flex: 1;
}
.env-item .env-tag {
  font-size: 10px;
  color: var(--text-3);
  border: 1px solid var(--border);
  padding: 1px 6px;
  border-radius: 10px;
}

.env-editor {
  border-top: 1px solid var(--border-2);
  padding: 10px;
  flex: 1;
  overflow-y: auto;
  min-height: 0;
}
.env-editor-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 8px;
}
.env-editor-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12.5px;
  color: var(--text);
}
.env-editor-title .dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--text-3);
}
.env-editor-title .dot.global { background: var(--purple); }
.env-editor-title .dot.dev    { background: var(--green); }
.env-editor-title .dot.stg    { background: var(--yellow); }
.env-editor-title .dot.prd    { background: var(--red); }
.env-editor-title .dot.custom { background: var(--accent); }
.env-editor-title .muted {
  color: var(--text-3);
  font-size: 11px;
  font-weight: 400;
}
.env-section-label {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 10px 0 6px;
  color: var(--text-2);
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.5px;
  text-transform: uppercase;
}
.env-section-label:first-of-type {
  margin-top: 0;
}
.env-section-label .muted {
  color: var(--text-3);
  font-size: 11px;
  font-weight: 400;
  letter-spacing: 0;
  text-transform: none;
}

/* === 历史 === */
.history-list {
  padding: 6px;
  overflow-y: auto;
  flex: 1;
  min-height: 0;
}
.history-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  border-radius: var(--radius);
  font-size: 12px;
  color: var(--text-2);
  cursor: pointer;
}
.history-item:hover {
  background: var(--bg-2);
  color: var(--text);
}
.history-item .hist-url {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: var(--mono);
  font-size: 11.5px;
}
.history-item .hist-status {
  font-family: var(--mono);
  font-size: 11px;
  padding: 1px 6px;
  border-radius: 3px;
  background: rgba(46, 194, 126, 0.12);
  color: var(--green);
}
.history-item .hist-status.err {
  background: rgba(240, 99, 99, 0.12);
  color: var(--red);
}

/* === 工作区 === */
.workspace {
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background: var(--bg);
  min-height: 0;
}

.tabs {
  display: flex;
  align-items: stretch;
  gap: 2px;
  background: var(--bg-2);
  border-bottom: 1px solid var(--border);
  padding: 6px 8px 0;
  height: 40px;
  overflow-x: auto;
  overflow-y: hidden;
  scrollbar-width: none;
  flex-shrink: 0;
}
.tabs::-webkit-scrollbar {
  display: none;
}
.tab {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 10px 0 12px;
  font-size: 12.5px;
  cursor: pointer;
  color: var(--text-2);
  position: relative;
  min-width: 148px;
  max-width: 220px;
  height: 34px;
  border-radius: 8px 8px 0 0;
  transition: background 0.15s ease, color 0.15s ease;
}
.tab:hover {
  background: color-mix(in srgb, var(--bg-3) 60%, transparent);
  color: var(--text);
}
.tab.active {
  background: var(--bg);
  color: var(--text);
  box-shadow: inset 0 0 0 1px var(--border);
  border-bottom: 1px solid var(--bg);
}
.tab.active::after {
  content: "";
  position: absolute;
  left: 10px;
  right: 10px;
  bottom: -1px;
  height: 2px;
  background: var(--primary);
  border-radius: 2px 2px 0 0;
}
.tab-title {
  flex: 1;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  font-weight: 500;
}
.tab-close {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: none;
  color: var(--text-3);
  cursor: pointer;
  width: 18px;
  height: 18px;
  border-radius: 5px;
  transition: background 0.15s ease, color 0.15s ease;
}
.tab-close:hover {
  background: color-mix(in srgb, var(--red) 14%, transparent);
  color: var(--red);
}
.tab.new {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 34px;
  width: 34px;
  padding: 0;
  color: var(--text-3);
  background: transparent;
  border: none;
  border-radius: 8px;
  cursor: pointer;
  transition: background 0.15s ease, color 0.15s ease;
}
.tab.new:hover {
  color: var(--primary);
  background: color-mix(in srgb, var(--primary) 12%, transparent);
}
.tab.new::after {
  display: none;
}

.empty-workspace {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
}
.empty-card {
  text-align: center;
  color: var(--text-2);
  background: var(--panel);
  border: 1px dashed var(--border);
  border-radius: 12px;
  padding: 36px 56px;
  max-width: 420px;
}
.empty-card h3 {
  margin: 0 0 8px;
  color: var(--text);
}
.empty-card p {
  margin: 0 0 16px;
  font-size: 12.5px;
}

/* 请求编辑区 */
.request-pane {
  padding: 12px 16px 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
  flex-shrink: 0;
  overflow: hidden;
  min-height: 180px;
}

.row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.breadcrumb-row {
  justify-content: space-between;
}
.breadcrumb {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  color: var(--text-2);
  user-select: text;
}
.breadcrumb .crumb {
  color: var(--text-2);
}
.breadcrumb .crumb.editable {
  color: var(--text);
  font-weight: 600;
  outline: none;
  padding: 2px 4px;
  border-radius: 3px;
  user-select: text;
}
.breadcrumb .crumb.editable:hover {
  background: var(--bg-2);
}
.breadcrumb .crumb.editable:focus {
  background: var(--bg-2);
  box-shadow: 0 0 0 1px var(--primary);
}
.breadcrumb .sep {
  color: var(--text-3);
}

.actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.send-group {
  display: flex;
  position: relative;
  flex-shrink: 0;
}
.send-group .btn {
  border-radius: 10px 0 0 10px;
  padding-left: 22px;
  padding-right: 22px;
  font-weight: 600;
  letter-spacing: 0.3px;
}
.send-group .btn.split {
  border-left: 1px solid rgba(0, 0, 0, 0.22);
  border-radius: 0 10px 10px 0;
  padding: 6px 10px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
.dropdown {
  position: absolute;
  top: calc(100% + 6px);
  right: 0;
  background: var(--bg-2);
  border: 1px solid var(--border);
  border-radius: 10px;
  min-width: 232px;
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.28);
  padding: 6px;
  z-index: 20;
}
.dropdown-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border-radius: 6px;
  font-size: 12.5px;
  cursor: pointer;
  color: var(--text);
  transition: background 0.12s ease, color 0.12s ease;
}
.dropdown-item:hover {
  background: var(--bg-hover);
}
.dropdown-item.disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
.dropdown-item .dd-icon {
  color: var(--primary);
  width: 18px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
.dropdown-item .kbd {
  margin-left: auto;
  font-family: var(--mono);
  font-size: 10.5px;
  color: var(--text-3);
  background: var(--bg);
  border: 1px solid var(--border);
  padding: 2px 6px;
  border-radius: 4px;
  letter-spacing: 0.4px;
}

.request-url-line {
  align-items: stretch;
}
.request-url-line > .url-row {
  flex: 1;
  min-width: 0;
}
.request-url-line > .send-group .btn {
  height: 100%;
}
.url-row {
  display: flex;
  align-items: center;
  background: var(--bg-2);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 0;
  position: relative;
  overflow: visible;
  transition: border-color 0.15s ease, box-shadow 0.15s ease, background 0.15s ease;
}
.url-row:hover {
  border-color: var(--border-2);
}
.url-row:focus-within {
  border-color: var(--primary);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--primary) 22%, transparent);
  background: var(--bg);
}
.select {
  position: relative;
}
.select select {
  appearance: none;
  background: transparent;
  border: none;
  outline: none;
  color: var(--text);
  padding: 8px 24px 8px 12px;
  font-family: var(--mono);
  font-size: 12px;
  font-weight: 700;
  cursor: pointer;
}
.select::after {
  content: "▾";
  position: absolute;
  right: 8px;
  top: 50%;
  transform: translateY(-50%);
  color: var(--text-3);
  font-size: 10px;
  pointer-events: none;
}
.method-select.get select { color: var(--green); }
.method-select.post select { color: var(--yellow); }
.method-select.put select { color: var(--blue); }
.method-select.patch select { color: var(--purple); }
.method-select.delete select { color: var(--red); }
.url-input {
  flex: 1;
}
.url-input input {
  width: 100%;
  background: transparent;
  border: none;
  outline: none;
  color: var(--text);
  font-family: var(--mono);
  font-size: 13px;
  padding: 10px 14px;
}
.url-input input::placeholder {
  color: var(--text-3);
}

/* 参数 Tab */
.param-tabs {
  display: flex;
  border-bottom: 1px solid var(--border);
  gap: 2px;
}
.param-tab {
  padding: 10px 14px;
  font-size: 12.5px;
  color: var(--text-2);
  cursor: pointer;
  border-bottom: 2px solid transparent;
  display: flex;
  align-items: center;
  gap: 6px;
  user-select: none;
  font-weight: 500;
  margin-bottom: -1px;
  transition: color 0.15s ease, border-color 0.15s ease;
}
.param-tab:hover {
  color: var(--text);
}
.param-tab.active {
  color: var(--primary);
  border-bottom-color: var(--primary);
}
.param-tab .badge {
  background: var(--bg-3);
  color: var(--text-2);
  border-radius: 999px;
  padding: 1px 7px;
  font-size: 10.5px;
  font-weight: 600;
  min-width: 18px;
  text-align: center;
}
.param-tab.active .badge {
  background: color-mix(in srgb, var(--primary) 20%, transparent);
  color: var(--primary);
}
.param-tabs.sm .param-tab {
  padding: 8px 12px;
  font-size: 12px;
}

.param-content {
  padding: 8px 0;
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}
.placeholder {
  color: var(--text-3);
  padding: 28px;
  text-align: center;
  font-size: 12.5px;
  border: 1px dashed var(--border);
  border-radius: 10px;
  background: color-mix(in srgb, var(--bg-2) 40%, transparent);
}

/* Tests editor + results */
.tests-panel {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 8px 0;
}
.tests-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
}
.tests-toolbar-label {
  font-size: 12px;
  color: var(--text-3);
  letter-spacing: 0.02em;
}
.tests-snippets {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}
.chip-btn {
  padding: 4px 10px;
  font-size: 12px;
  border-radius: 999px;
  border: 1px solid var(--border-2);
  background: var(--bg-2);
  color: var(--text-2);
  cursor: pointer;
  transition: background 0.15s ease, border-color 0.15s ease, color 0.15s ease;
}
.chip-btn:hover {
  background: color-mix(in srgb, var(--primary) 10%, var(--bg-2));
  border-color: color-mix(in srgb, var(--primary) 40%, var(--border-2));
  color: var(--text-1);
}
.chip-btn.ghost {
  background: transparent;
  color: var(--text-3);
}
.tests-editor-wrap {
  position: relative;
  flex: 1 1 132px;
  min-height: 132px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--bg-2);
  overflow: hidden;
  transition: border-color 0.15s ease, box-shadow 0.15s ease;
}
.tests-editor-wrap:focus-within {
  border-color: var(--primary);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--primary) 22%, transparent);
}
.tests-highlight,
.tests-editor {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  margin: 0;
  padding: 12px 14px;
  box-sizing: border-box;
  font-family: var(--mono);
  font-size: 12.5px;
  line-height: 1.55;
  white-space: pre;
  word-wrap: normal;
  word-break: normal;
  tab-size: 2;
  border: none;
  outline: none;
  background: transparent;
  letter-spacing: 0;
}
.tests-highlight {
  pointer-events: none;
  color: var(--text-1);
  z-index: 1;
  overflow: hidden;
  scrollbar-width: none;
}
.tests-highlight::-webkit-scrollbar {
  display: none;
}
.tests-editor {
  z-index: 2;
  resize: none;
  overflow: auto;
  color: transparent !important;
  caret-color: var(--primary);
  -webkit-text-fill-color: transparent !important;
}
.tests-editor::selection {
  background: color-mix(in srgb, var(--primary) 35%, transparent);
  color: transparent;
  -webkit-text-fill-color: transparent;
}
.tests-editor::-moz-selection {
  background: color-mix(in srgb, var(--primary) 35%, transparent);
  color: transparent;
}
.tests-editor:placeholder-shown {
  color: var(--text-3) !important;
  -webkit-text-fill-color: var(--text-3) !important;
}
.tests-editor::placeholder {
  color: var(--text-3);
  -webkit-text-fill-color: var(--text-3);
  white-space: pre;
}

/* JS syntax highlighting tokens */
.tk-c { color: var(--text-3); font-style: italic; }
.tk-s { color: #8bd17c; }
.tk-r { color: #f79e6c; }
.tk-n { color: #c586c0; }
.tk-k { color: #c586c0; font-weight: 600; }
.tk-l { color: #d19a66; }
.tk-b { color: #4fc1ff; font-weight: 600; }
.tk-i { color: var(--text-1); }
.tk-p { color: var(--text-2); }
.tk-op { color: var(--text-2); }
[data-theme="light"] .tk-c { color: #6a737d; }
[data-theme="light"] .tk-s { color: #22863a; }
[data-theme="light"] .tk-r { color: #d15704; }
[data-theme="light"] .tk-n { color: #6f42c1; }
[data-theme="light"] .tk-k { color: #d73a49; }
[data-theme="light"] .tk-l { color: #005cc5; }
[data-theme="light"] .tk-b { color: #005cc5; }
[data-theme="light"] .tk-i { color: var(--text-1); }
[data-theme="light"] .tk-p { color: var(--text-2); }
[data-theme="light"] .tk-op { color: #24292e; }

.tests-hint {
  font-size: 11.5px;
  color: var(--text-3);
  line-height: 1.6;
}
.tests-hint code {
  padding: 1px 6px;
  border-radius: 4px;
  background: color-mix(in srgb, var(--bg-3) 60%, transparent);
  font-family: var(--mono);
  font-size: 11px;
  color: var(--text-2);
}

.tests-results {
  padding: 12px 4px;
}
.tests-summary {
  display: flex;
  gap: 8px;
  margin-bottom: 12px;
  flex-wrap: wrap;
}
.tests-summary .chip {
  padding: 3px 10px;
  border-radius: 999px;
  font-size: 12px;
  border: 1px solid transparent;
  color: var(--text-2);
  background: color-mix(in srgb, var(--bg-3) 60%, transparent);
}
.tests-summary .chip.ok {
  color: var(--green);
  background: color-mix(in srgb, var(--green) 14%, transparent);
  border-color: color-mix(in srgb, var(--green) 35%, transparent);
}
.tests-summary .chip.err {
  color: var(--red);
  background: color-mix(in srgb, var(--red) 14%, transparent);
  border-color: color-mix(in srgb, var(--red) 35%, transparent);
}
.badge.err {
  color: var(--red);
  background: color-mix(in srgb, var(--red) 16%, transparent);
}
.tests-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.tests-item {
  padding: 8px 12px;
  border: 1px solid var(--border);
  border-left: 3px solid transparent;
  border-radius: 8px;
  background: var(--bg-2);
}
.tests-item.ok {
  border-left-color: var(--green);
}
.tests-item.err {
  border-left-color: var(--red);
  background: color-mix(in srgb, var(--red) 6%, var(--bg-2));
}
.tests-item-head {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12.5px;
}
.tests-status {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  flex-shrink: 0;
}
.tests-item.ok .tests-status {
  color: var(--green);
  background: color-mix(in srgb, var(--green) 18%, transparent);
}
.tests-item.err .tests-status {
  color: var(--red);
  background: color-mix(in srgb, var(--red) 18%, transparent);
}
.tests-name {
  flex: 1;
  color: var(--text-1);
  word-break: break-word;
}
.tests-duration {
  font-size: 11px;
  color: var(--text-3);
  font-family: var(--mono);
}
.tests-error {
  margin: 8px 0 0;
  padding: 8px 10px;
  font-family: var(--mono);
  font-size: 11.5px;
  color: var(--red);
  background: color-mix(in srgb, var(--red) 10%, transparent);
  border-radius: 6px;
  white-space: pre-wrap;
  overflow-x: auto;
}

/* KV 表格 */
.kv-table {
  width: 100%;
  border-collapse: separate;
  border-spacing: 0;
  font-size: 12.5px;
  border: 1px solid var(--border);
  border-radius: 10px;
  overflow: hidden;
  background: var(--bg-2);
}
.kv-table th {
  text-align: left;
  background: color-mix(in srgb, var(--bg-3) 60%, transparent);
  color: var(--text-3);
  font-weight: 600;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border);
  font-size: 10.5px;
  text-transform: uppercase;
  letter-spacing: 0.6px;
}
.kv-table td {
  padding: 0;
  border-bottom: 1px solid var(--border-2);
}
.kv-table tr:last-child td {
  border-bottom: none;
}
.kv-table td.check {
  width: 34px;
  text-align: center;
  padding: 0 8px;
}
.kv-table td input[type="text"] {
  width: 100%;
  background: transparent;
  border: none;
  outline: none;
  color: var(--text);
  padding: 9px 12px;
  font-family: var(--mono);
  font-size: 12px;
  transition: background 0.12s ease, box-shadow 0.12s ease;
}
.kv-table td input[type="text"]:focus {
  background: color-mix(in srgb, var(--primary) 8%, transparent);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--primary) 45%, transparent);
}
.kv-table td input[type="text"]::placeholder {
  color: var(--text-3);
}
.kv-table td input[type="checkbox"] {
  accent-color: var(--primary);
  width: 14px;
  height: 14px;
  cursor: pointer;
}
.kv-table td .icon-btn {
  margin: 0 4px;
}
.kv-table tr:hover {
  background: color-mix(in srgb, var(--row-hover) 55%, transparent);
}
.kv-table.compact th {
  padding: 6px 10px;
}
.kv-table.compact td input[type="text"] {
  padding: 6px 10px;
  font-size: 11.5px;
}

/* Body 类型 */
.body-types {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 8px 4px 12px;
  flex-wrap: wrap;
}
.radio {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
  font-size: 12.5px;
  color: var(--text-2);
  transition: color 0.15s ease;
}
.radio:hover {
  color: var(--text);
}
.radio input {
  accent-color: var(--primary);
  width: 14px;
  height: 14px;
}
.raw-select {
  background: var(--bg-2);
  border: 1px solid var(--border);
  border-radius: 8px;
}
.raw-select select {
  padding: 5px 24px 5px 10px;
  font-size: 12px;
  font-weight: 500;
  color: var(--text);
}
.body-actions {
  margin-left: auto;
  display: flex;
  gap: 6px;
  align-items: center;
}
.body-action-btn {
  height: 28px;
  padding: 0 12px;
  font-size: 11.5px;
  font-weight: 500;
  letter-spacing: 0.2px;
  border: 1px solid var(--border);
  color: var(--text-2);
  border-radius: 7px;
  background: transparent;
  transition: color 0.15s ease, border-color 0.15s ease, background 0.15s ease;
}
.body-action-btn:hover {
  color: var(--primary);
  border-color: var(--primary);
  background: color-mix(in srgb, var(--primary) 10%, transparent);
}
.body-action-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
  color: var(--text-3);
  border-color: var(--border-2);
  background: transparent;
}
.body-action-btn:disabled:hover {
  color: var(--text-3);
  border-color: var(--border-2);
  background: transparent;
}
.body-table-wrap {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.body-kv-table .body-type-col {
  width: 116px;
}
.field-type-switch {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  margin: 4px 8px;
  padding: 2px;
  background: var(--bg);
  border: 1px solid var(--border-2);
  border-radius: 999px;
}
.field-type-switch button {
  border: none;
  border-radius: 999px;
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
  font-size: 11px;
  font-weight: 600;
  line-height: 1;
  padding: 5px 9px;
  transition: background 0.15s, color 0.15s, box-shadow 0.15s;
}
.field-type-switch button:hover {
  color: var(--text);
}
.field-type-switch button.active {
  background: var(--bg-3);
  color: var(--primary);
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.16);
}
.file-cell,
.binary-picker {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
.file-cell {
  padding: 3px 8px;
}
.binary-picker {
  border: 1px dashed var(--border);
  border-radius: var(--radius);
  padding: 18px;
}
.file-path {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-2);
  font-family: var(--mono);
  font-size: 12px;
}
.hint-line {
  color: var(--text-3);
  font-size: 12px;
  padding: 0 2px;
}

/* 代码编辑器 */
.code-editor {
  display: flex;
  background: var(--code-bg);
  border: 1px solid var(--border);
  border-radius: 10px;
  overflow: hidden;
  font-family: var(--mono);
  font-size: 12.5px;
  min-height: 180px;
  transition: border-color 0.15s ease, box-shadow 0.15s ease;
}
.code-editor:focus-within {
  border-color: var(--primary);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--primary) 22%, transparent);
}
.code-editor.preview {
  padding: 0;
  min-height: 240px;
}
.preview-frame {
  flex: 1;
  border: 0;
  background: #fff;
  width: 100%;
  min-height: 240px;
}
.line-numbers {
  background: var(--code-gutter);
  color: var(--text-3);
  padding: 10px 8px;
  text-align: right;
  border-right: 1px solid var(--border-2);
  display: flex;
  flex-direction: column;
  gap: 2px;
  user-select: none;
  min-width: 36px;
  min-height: 0;
  overflow: hidden;
  flex-shrink: 0;
}
.line-numbers span {
  line-height: 1.5;
  font-size: 11px;
}
.line-numbers.foldable {
  min-width: 52px;
  padding-left: 4px;
  gap: 0;
}
.line-numbers.foldable span {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 3px;
  line-height: 18.75px;
}
.json-fold-btn {
  flex: 0 0 12px;
  width: 12px;
  height: 14px;
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
  font-size: 10px;
  line-height: 1;
}
.json-fold-btn:hover {
  color: var(--primary);
}
.json-fold-spacer {
  flex: 0 0 12px;
  width: 12px;
  display: inline-block;
}
.json-fold-ph {
  color: var(--text-3);
  cursor: pointer;
  border-radius: 3px;
  padding: 0 2px;
}
.json-fold-ph:hover {
  color: var(--primary);
  background: rgba(255, 122, 61, 0.14);
}
.code.interactive {
  cursor: text;
}
.code {
  margin: 0;
  padding: 10px 14px;
  color: var(--text);
  line-height: 1.5;
  flex: 1;
  white-space: pre;
  overflow: auto;
}
.code.muted {
  color: var(--text-3);
}
.code .key {
  color: var(--code-key);
}
.code .str {
  color: var(--code-str);
}
.code .num {
  color: var(--code-num);
}
.code .bool {
  color: var(--code-bool);
}
.code .punc {
  color: var(--code-punc);
}

/* Body 编辑器：textarea 透明叠在 pre 上，pre 渲染高亮 */
.body-editor .line-numbers {
  gap: 0;
}
.body-editor .line-numbers span {
  line-height: 18.75px;
}
.body-editor .body-edit {
  position: relative;
  flex: 1;
  min-height: 180px;
  overflow: hidden;
}
.body-edit .body-highlight,
.body-edit .body-textarea {
  margin: 0;
  width: 100%;
  box-sizing: border-box;
  font-family: var(--mono);
  font-size: 12.5px;
  line-height: 1.5;
  padding: 10px 14px;
  white-space: pre;
  word-break: normal;
  overflow-wrap: normal;
  border: 0;
  letter-spacing: 0;
  tab-size: 2;
}
.body-edit .body-highlight {
  position: relative;
  pointer-events: none;
  color: var(--text);
  background: transparent;
  overflow-x: auto;
  overflow-y: hidden;
  scrollbar-width: none;
}
.body-edit .body-highlight.interactive {
  pointer-events: auto;
}
.body-edit .body-highlight::-webkit-scrollbar {
  display: none;
}
.body-edit .body-textarea {
  position: absolute;
  inset: 0;
  background: transparent;
  color: transparent;
  caret-color: var(--text);
  outline: none;
  resize: none;
  overflow-x: auto;
  overflow-y: hidden;
}
.body-edit .body-textarea::selection {
  background: rgba(255, 122, 61, 0.28);
  color: transparent;
}
.body-edit .body-textarea::placeholder {
  color: var(--text-3);
  -webkit-text-fill-color: var(--text-3);
}
/* 关键：textarea 文字隐形（保留光标） */
.body-edit .body-textarea {
  -webkit-text-fill-color: transparent;
}

/* === 拖拽分割条 === */
.splitter {
  height: 8px;
  background: transparent;
  cursor: row-resize;
  margin-top: 4px;
  flex-shrink: 0;
  position: relative;
  z-index: 4;
  display: flex;
  align-items: center;
  justify-content: center;
}
.splitter::after {
  content: "";
  position: absolute;
  left: 0;
  right: 0;
  top: 50%;
  height: 1px;
  background: var(--border-2);
  transition: background 0.15s;
}
.splitter .grip {
  position: relative;
  z-index: 1;
  width: 36px;
  height: 4px;
  background: var(--border);
  border-radius: 2px;
  transition: background 0.15s, transform 0.15s;
}
.splitter:hover .grip,
.splitter:active .grip {
  background: var(--primary);
  transform: scaleX(1.15);
}
.splitter:hover::after,
.splitter:active::after {
  background: var(--primary);
}

/* === 响应区 === */
.response-pane {
  padding: 10px 16px 16px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  flex: 1 1 0;
  min-height: 0;
  overflow: hidden;
}
.response-meta {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-wrap: wrap;
  gap: 10px;
  padding: 8px 12px;
  background: var(--bg-2);
  border: 1px solid var(--border);
  border-radius: 10px;
}
.meta-left {
  display: flex;
  align-items: center;
  gap: 16px;
  font-size: 12px;
  color: var(--text-2);
  flex-wrap: wrap;
}
.meta-item b {
  color: var(--text);
  font-weight: 600;
}
.status {
  font-family: var(--mono);
  font-weight: 700;
  font-size: 11.5px;
  padding: 4px 10px;
  border-radius: 999px;
  letter-spacing: 0.3px;
  border: 1px solid transparent;
}
.status.ok {
  color: var(--green);
  background: color-mix(in srgb, var(--green) 14%, transparent);
  border-color: color-mix(in srgb, var(--green) 22%, transparent);
}
.status.err {
  color: var(--red);
  background: color-mix(in srgb, var(--red) 14%, transparent);
  border-color: color-mix(in srgb, var(--red) 22%, transparent);
}
.status.idle {
  color: var(--text-3);
  background: var(--bg-3);
  border-color: var(--border-2);
}
.meta-right {
  display: flex;
  gap: 6px;
  align-items: center;
}

.response-body {
  display: flex;
  flex-direction: column;
  gap: 8px;
  flex: 1;
  min-height: 0;
}
.response-body .code-editor {
  flex: 1;
}
.resp-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.seg {
  display: inline-flex;
  background: var(--bg-2);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 2px;
  gap: 2px;
}
.seg-btn {
  background: transparent;
  border: none;
  padding: 5px 12px;
  color: var(--text-2);
  font-size: 11.5px;
  font-weight: 500;
  cursor: pointer;
  border-radius: 6px;
  transition: background 0.15s ease, color 0.15s ease;
}
.seg-btn:hover {
  color: var(--text);
  background: color-mix(in srgb, var(--bg-3) 60%, transparent);
}
.seg-btn.active {
  background: var(--bg);
  color: var(--primary);
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.18);
}
.select.sm {
  background: var(--bg-2);
  border: 1px solid var(--border);
  border-radius: 8px;
}
.select.sm select {
  padding: 5px 22px 5px 10px;
  font-size: 12px;
  font-weight: 500;
}

/* === 状态栏 === */
.statusbar {
  height: 24px;
  background: var(--statusbar-bg);
  border-top: 1px solid var(--border-2);
  display: flex;
  align-items: center;
  padding: 0 12px;
  font-size: 11px;
  color: var(--text-3);
  gap: 8px;
  cursor: default;
}
.statusbar > * {
  pointer-events: none;
}
.statusbar .status-dot {
  color: var(--green);
}
.statusbar .dot-sep {
  color: var(--border);
}
.status-right {
  margin-left: auto;
  display: flex;
  gap: 8px;
  align-items: center;
}

/* === 自定义下拉 === */
.custom-select {
  position: relative;
  display: inline-flex;
  font-family: var(--sans);
}
.cs-trigger {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  background: transparent;
  border: none;
  outline: none;
  color: var(--text);
  padding: 8px 10px 8px 12px;
  font-family: var(--sans);
  font-size: 12.5px;
  font-weight: 600;
  cursor: pointer;
  border-radius: 8px;
  transition: background 0.15s ease, color 0.15s ease;
}
.cs-trigger:hover {
  background: color-mix(in srgb, var(--bg-hover) 70%, transparent);
}
.custom-select.open .cs-trigger {
  background: color-mix(in srgb, var(--bg-hover) 70%, transparent);
}
.cs-trigger.sm {
  padding: 4px 8px 4px 10px;
  font-size: 11.5px;
  font-weight: 500;
}
.cs-caret {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  transition: transform 0.18s ease, color 0.15s ease;
}
.custom-select.open .cs-caret {
  transform: rotate(-180deg);
  color: var(--primary);
}
.method-select {
  min-width: 108px;
  padding: 4px 4px 4px 6px;
  align-items: stretch;
  border-right: none;
  position: relative;
}
.method-select::after {
  content: "";
  position: absolute;
  right: 0;
  top: 22%;
  bottom: 22%;
  width: 1px;
  background: var(--border);
}
.method-select .cs-trigger {
  width: 100%;
  gap: 8px;
  padding: 7px 10px 7px 12px;
  border-radius: 8px;
  font-family: var(--mono);
  font-weight: 700;
  font-size: 12px;
  letter-spacing: 0.4px;
  background: color-mix(in srgb, var(--text-3) 12%, transparent);
  color: var(--text);
  transition: background 0.18s ease, color 0.18s ease;
}
.method-select .cs-trigger:hover {
  background: color-mix(in srgb, var(--text-3) 20%, transparent);
}
.method-select.open .cs-trigger {
  background: color-mix(in srgb, var(--text-3) 22%, transparent);
}
.method-select .cs-value {
  flex: 1;
  text-align: left;
}
.method-select .dot-method {
  display: none;
}
.method-select.get .cs-trigger {
  background: color-mix(in srgb, var(--green) 14%, transparent);
  color: var(--green);
}
.method-select.get .cs-trigger:hover,
.method-select.get.open .cs-trigger {
  background: color-mix(in srgb, var(--green) 22%, transparent);
}
.method-select.post .cs-trigger {
  background: color-mix(in srgb, var(--yellow) 14%, transparent);
  color: var(--yellow);
}
.method-select.post .cs-trigger:hover,
.method-select.post.open .cs-trigger {
  background: color-mix(in srgb, var(--yellow) 22%, transparent);
}
.method-select.put .cs-trigger {
  background: color-mix(in srgb, var(--blue) 14%, transparent);
  color: var(--blue);
}
.method-select.put .cs-trigger:hover,
.method-select.put.open .cs-trigger {
  background: color-mix(in srgb, var(--blue) 22%, transparent);
}
.method-select.patch .cs-trigger {
  background: color-mix(in srgb, var(--purple) 14%, transparent);
  color: var(--purple);
}
.method-select.patch .cs-trigger:hover,
.method-select.patch.open .cs-trigger {
  background: color-mix(in srgb, var(--purple) 22%, transparent);
}
.method-select.delete .cs-trigger {
  background: color-mix(in srgb, var(--red) 14%, transparent);
  color: var(--red);
}
.method-select.delete .cs-trigger:hover,
.method-select.delete.open .cs-trigger {
  background: color-mix(in srgb, var(--red) 22%, transparent);
}
.method-select .cs-caret {
  color: currentColor;
  opacity: 0.7;
}
.method-select .cs-menu {
  min-width: 140px;
  font-family: var(--mono);
}
.method-select .cs-item {
  font-weight: 600;
  letter-spacing: 0.3px;
  font-size: 12px;
}
.method-select.get .cs-value,
.method-select.post .cs-value,
.method-select.put .cs-value,
.method-select.patch .cs-value,
.method-select.delete .cs-value {
  color: inherit;
}
.raw-select {
  background: transparent;
  border: 1px solid transparent;
  border-radius: 999px;
  transition: background 0.15s ease, border-color 0.15s ease;
}
.raw-select:hover {
  background: color-mix(in srgb, var(--bg-hover) 70%, transparent);
  border-color: var(--border-2);
}
.raw-select.open {
  background: color-mix(in srgb, var(--bg-hover) 70%, transparent);
  border-color: var(--border-2);
}
.raw-select .cs-trigger.sm {
  padding: 4px 8px 4px 6px;
  gap: 6px;
  border-radius: 999px;
  background: transparent;
  font-family: var(--sans);
  font-weight: 500;
  font-size: 11.5px;
  color: var(--text-2);
}
.raw-select .cs-trigger.sm:hover {
  background: transparent;
  color: var(--text);
}
.raw-select .cs-caret {
  color: var(--text-3);
}
.raw-select.open .cs-caret {
  color: var(--primary);
}
.raw-select .cs-menu {
  min-width: 160px;
}
.custom-select.sm {
  background: var(--bg-2);
  border: 1px solid var(--border);
  border-radius: var(--radius);
}
.cs-menu {
  position: absolute;
  top: calc(100% + 6px);
  left: 0;
  background: var(--bg-2);
  border: 1px solid var(--border);
  border-radius: 10px;
  min-width: 172px;
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.28);
  padding: 6px;
  z-index: 30;
  font-family: var(--sans);
}
.cs-menu.align-end {
  left: auto;
  right: 0;
}
.cs-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border-radius: 6px;
  font-size: 12.5px;
  color: var(--text);
  cursor: pointer;
  user-select: none;
  transition: background 0.12s ease, color 0.12s ease;
}
.cs-item:hover {
  background: var(--bg-hover);
}
.cs-item.active {
  background: color-mix(in srgb, var(--primary) 14%, transparent);
  color: var(--primary);
  font-weight: 500;
}
.dot-method {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  display: inline-block;
  flex-shrink: 0;
  background: var(--text-3);
  box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.04) inset;
}
.dot-method.get { background: var(--green); }
.dot-method.post { background: var(--yellow); }
.dot-method.put { background: var(--blue); }
.dot-method.patch { background: var(--purple); }
.dot-method.delete { background: var(--red); }

.lang-tag {
  font-family: var(--mono);
  font-size: 10px;
  font-weight: 700;
  padding: 2px 7px;
  border-radius: 999px;
  letter-spacing: 0.5px;
  color: #fff;
  display: inline-flex;
  align-items: center;
  min-width: 34px;
  justify-content: center;
}
.lang-tag.json { background: #d4843a; }
.lang-tag.xml { background: #4aa9ff; color: #0b1424; }
.lang-tag.html { background: #f06363; }
.lang-tag.text { background: #6c7088; }
.lang-tag.js { background: #f5b942; color: #1e1f24; }
.lang-tag.form { background: #2ec27e; }

.url-input {
  flex: 1;
  display: flex;
  align-items: center;
  position: relative;
}
.url-input input {
  flex: 1;
  min-width: 0;
}

/* === 导入 cURL 弹窗 === */
.modal-mask {
  position: fixed;
  inset: 0;
  background: var(--modal-mask);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
  backdrop-filter: blur(4px);
  animation: modal-mask-in 0.15s ease-out;
}
@keyframes modal-mask-in {
  from { opacity: 0; }
  to   { opacity: 1; }
}
.modal-card {
  width: min(680px, 92vw);
  background: var(--bg-2);
  border: 1px solid var(--border);
  border-radius: 14px;
  box-shadow: 0 24px 60px rgba(0, 0, 0, 0.32);
  display: flex;
  flex-direction: column;
  max-height: 86vh;
  overflow: hidden;
  animation: modal-card-in 0.18s ease-out;
}
@keyframes modal-card-in {
  from { opacity: 0; transform: translateY(10px) scale(0.97); }
  to   { opacity: 1; transform: translateY(0) scale(1); }
}
.import-card {
  width: min(720px, 92vw);
}
.modal-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 18px;
  font-size: 14px;
  color: var(--text);
}
.modal-title {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  color: var(--text);
  font-size: 14px;
}
.modal-title strong {
  font-weight: 600;
  letter-spacing: 0.2px;
}
.modal-title-icon {
  width: 28px;
  height: 28px;
  border-radius: 8px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: color-mix(in srgb, var(--primary) 14%, transparent);
  color: var(--primary);
}
.modal-close {
  width: 26px;
  height: 26px;
  border: none;
  border-radius: 8px;
  background: transparent;
  color: var(--text-3);
  font-size: 18px;
  line-height: 1;
  cursor: pointer;
  transition: background 0.15s ease, color 0.15s ease;
}
.modal-close:hover {
  background: var(--bg-3);
  color: var(--text);
}
.modal-tabs {
  display: flex;
  gap: 4px;
  padding: 0 18px;
  border-bottom: 1px solid var(--border-2);
  background: transparent;
}
.m-tab {
  padding: 10px 14px;
  font-size: 12.5px;
  color: var(--text-2);
  cursor: pointer;
  border-bottom: 2px solid transparent;
  user-select: none;
  transition: color 0.15s ease, border-color 0.15s ease;
}
.m-tab:hover {
  color: var(--text);
}
.m-tab.active {
  color: var(--primary);
  border-bottom-color: var(--primary);
  font-weight: 500;
}
.m-tab.disabled {
  color: var(--text-3);
  cursor: not-allowed;
  position: relative;
}
.m-tab.disabled:hover {
  color: var(--text-3);
}
.m-tab.disabled::after {
  content: "soon";
  font-size: 9px;
  margin-left: 6px;
  padding: 1px 6px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--primary) 12%, transparent);
  color: var(--primary);
  letter-spacing: 0.4px;
  text-transform: uppercase;
  font-weight: 500;
}
.modal-body {
  padding: 18px 20px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  overflow-y: auto;
}
.form-label {
  font-size: 11.5px;
  color: var(--text-3);
  text-transform: uppercase;
  letter-spacing: 0.6px;
  font-weight: 500;
}
.modal-section-label {
  margin-top: 6px;
}
.curl-input-wrap {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.curl-status {
  align-self: flex-start;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 5px 12px 5px 10px;
  border-radius: 999px;
  font-size: 11.5px;
  letter-spacing: 0.2px;
  font-weight: 500;
  border: 1px solid transparent;
  transition: background 0.15s ease, color 0.15s ease;
}
.curl-status .status-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
}
.curl-status.valid {
  background: rgba(46, 194, 126, 0.12);
  color: var(--green);
}
.curl-status.valid .status-dot {
  background: var(--green);
  box-shadow: 0 0 6px rgba(46, 194, 126, 0.7);
}
.curl-status.invalid {
  background: rgba(240, 99, 99, 0.12);
  color: var(--red);
}
.curl-status.invalid .status-dot {
  background: var(--red);
  box-shadow: 0 0 6px rgba(240, 99, 99, 0.7);
}
.curl-status.muted {
  background: var(--bg-3);
  color: var(--text-3);
}
.curl-status.muted .status-dot {
  background: var(--text-3);
}

.parse-preview {
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.pp-title {
  font-size: 10.5px;
  text-transform: uppercase;
  letter-spacing: 0.6px;
  color: var(--text-3);
  margin-bottom: 2px;
}
.pp-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12.5px;
  flex-wrap: wrap;
}
.pp-label {
  width: 56px;
  color: var(--text-3);
  font-size: 11.5px;
  flex-shrink: 0;
}
.pp-url {
  font-family: var(--mono);
  color: var(--text);
  word-break: break-all;
  font-size: 12px;
}
.pp-tag {
  background: var(--bg-2);
  border: 1px solid var(--border);
  font-size: 11px;
  padding: 1px 8px;
  border-radius: 999px;
  color: var(--text-2);
  font-family: var(--mono);
  letter-spacing: 0.2px;
}
.pp-tag.json {
  background: rgba(212, 132, 58, 0.16);
  border-color: transparent;
  color: #f0a55c;
}
.pp-tag.xml {
  background: rgba(74, 169, 255, 0.16);
  border-color: transparent;
  color: var(--blue);
}
.pp-tag.html {
  background: rgba(240, 99, 99, 0.16);
  border-color: transparent;
  color: var(--red);
}
.pp-tag.js {
  background: rgba(245, 185, 66, 0.16);
  border-color: transparent;
  color: var(--yellow);
}
.pp-tag.text {
  background: rgba(168, 172, 187, 0.14);
  border-color: transparent;
  color: var(--text-2);
}
.pp-tag.form {
  background: rgba(46, 194, 126, 0.14);
  border-color: transparent;
  color: var(--green);
}
.pp-empty {
  color: var(--text-3);
  font-size: 12px;
  font-style: italic;
}

.form-row {
  display: flex;
  gap: 10px;
  align-items: center;
}
.col-cdrop {
  flex: 1;
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 8px;
  transition: border-color 0.15s ease, box-shadow 0.15s ease;
}
.col-cdrop:hover {
  border-color: var(--border-2);
}
.col-cdrop.open {
  border-color: var(--primary);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--primary) 22%, transparent);
}
.col-trigger {
  width: 100%;
  justify-content: flex-start;
  font-family: var(--sans);
  font-weight: 500;
  font-size: 12.5px;
  padding: 8px 10px 8px 12px;
}
.col-trigger .cs-value {
  flex: 1;
  text-align: left;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.col-trigger .cs-caret {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  transition: transform 0.18s ease, color 0.15s ease;
}
.col-cdrop.open .col-trigger .cs-caret {
  transform: rotate(-180deg);
  color: var(--primary);
}
.col-trigger .folder-icon,
.col-trigger .dd-icon {
  flex-shrink: 0;
}
.col-menu {
  width: 100%;
  min-width: 220px;
  max-height: 240px;
  overflow-y: auto;
}
.cs-divider {
  height: 1px;
  background: var(--border-2);
  margin: 4px 0;
}
.cs-item .folder-icon {
  color: var(--text-2);
}
.cs-item.action {
  color: var(--primary);
}
.cs-item .dd-icon {
  color: var(--primary);
  width: 16px;
  text-align: center;
}
.new-folder-input {
  margin-top: 4px;
}
.modal-foot {
  padding: 14px 20px;
  border-top: 1px solid var(--border);
  background: color-mix(in srgb, var(--bg-2) 60%, transparent);
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
.modal-label {
  font-size: 12px;
  color: var(--text-2);
}
.curl-input {
  background: var(--code-bg);
  border: 1px solid var(--border);
  border-radius: 10px;
  color: var(--text);
  font-family: var(--mono);
  font-size: 12px;
  line-height: 1.5;
  padding: 12px 14px;
  resize: vertical;
  min-height: 140px;
  outline: none;
  transition: border-color 0.15s ease, box-shadow 0.15s ease;
}
.curl-input:focus {
  border-color: var(--primary);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--primary) 22%, transparent);
}
.curl-status {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  font-size: 12px;
}
.curl-status .muted {
  color: var(--text-3);
}
.curl-meta {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  max-width: 100%;
  overflow: hidden;
}
.curl-url {
  font-family: var(--mono);
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 360px;
}
.modal-row {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.modal-row-fields {
  display: flex;
  gap: 8px;
  align-items: center;
}
.native-select,
.modal-input {
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 8px;
  color: var(--text);
  padding: 8px 12px;
  font-size: 13px;
  outline: none;
  min-width: 0;
  transition: border-color 0.15s ease, box-shadow 0.15s ease;
}
.native-select {
  font-family: var(--sans);
  flex: 1;
}
.modal-input {
  flex: 1;
  font-family: var(--sans);
}
.native-select::placeholder,
.modal-input::placeholder {
  color: var(--text-3);
}
.native-select:hover,
.modal-input:hover {
  border-color: var(--border-2);
}
.native-select:focus,
.modal-input:focus {
  border-color: var(--primary);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--primary) 22%, transparent);
}

/* 滚动条 */
::-webkit-scrollbar {
  width: 8px;
  height: 8px;
}
::-webkit-scrollbar-thumb {
  background: var(--border);
  border-radius: 4px;
}
::-webkit-scrollbar-thumb:hover {
  background: var(--text-3);
}
::-webkit-scrollbar-track {
  background: transparent;
}

/* === 拆分按钮：新建 HTTP / gRPC === */
.new-split {
  position: relative;
  display: inline-flex;
  align-items: stretch;
}
.new-split .new-main {
  border-top-right-radius: 0;
  border-bottom-right-radius: 0;
  border-right: 1px solid rgba(255, 255, 255, 0.18);
}
.new-split .new-caret {
  padding: 0 8px;
  border-top-left-radius: 0;
  border-bottom-left-radius: 0;
  border-left: none;
  min-width: 24px;
}
.new-split.inline {
  margin-top: 8px;
}
.new-menu {
  position: absolute;
  top: calc(100% + 4px);
  right: 0;
  min-width: 180px;
  z-index: 30;
  background: var(--bg-2);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  box-shadow: var(--shadow);
  padding: 4px;
  display: flex;
  flex-direction: column;
}
.new-menu .cs-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  border-radius: var(--radius-sm);
  cursor: pointer;
  font-size: 12px;
  color: var(--text);
}
.new-menu .cs-item:hover {
  background: var(--bg-hover);
}

/* === gRPC method 徽标 === */
.method.grpc {
  background: rgba(176, 124, 255, 0.18);
  color: var(--purple);
}
.tab .method.grpc,
.tree-row .method.grpc {
  background: rgba(176, 124, 255, 0.16);
  color: var(--purple);
}

/* === gRPC 顶部行 === */
.url-row.grpc {
  display: flex;
  align-items: center;
  gap: 8px;
}
.grpc-tag {
  background: rgba(176, 124, 255, 0.18);
  color: var(--purple);
  font-size: 11px;
  font-weight: 700;
  padding: 4px 10px;
  border-radius: var(--radius);
  letter-spacing: 0.5px;
}
.grpc-target-input {
  flex: 1;
  background: transparent;
  border: none;
  color: var(--text);
  padding: 10px 14px;
  font-family: var(--mono);
  font-size: 13px;
  outline: none;
}
.grpc-target-input::placeholder {
  color: var(--text-3);
}
.grpc-target-input:focus {
  outline: none;
}
.grpc-tls {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  color: var(--text-2);
  font-size: 12px;
  user-select: none;
  cursor: pointer;
}

/* === gRPC Message 编辑器 === */
.grpc-call-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border-2);
}
.grpc-input {
  flex: 1;
  background: var(--bg-2);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  color: var(--text);
  padding: 6px 10px;
  font-family: var(--mono);
  font-size: 12px;
}
.grpc-input:focus {
  border-color: var(--primary);
  outline: none;
}
.grpc-sep {
  color: var(--text-3);
  font-weight: 600;
}
.grpc-message-editor {
  min-height: 160px;
}

.grpc-select {
  flex: 1;
  min-width: 0;
}
.grpc-select .cs-trigger {
  width: 100%;
  background: var(--bg-2);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  color: var(--text);
  padding: 6px 10px;
  font-family: var(--mono);
  font-size: 12px;
  display: flex;
  align-items: center;
  gap: 6px;
}
.grpc-select .cs-trigger:hover {
  border-color: var(--primary);
}
.grpc-select .cs-trigger:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}
.grpc-select .cs-value {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  text-align: left;
}
.grpc-menu {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  min-width: 240px;
  max-width: 420px;
  max-height: 320px;
  overflow-y: auto;
  z-index: 30;
  background: var(--bg-2);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  box-shadow: var(--shadow);
  padding: 4px;
}
.grpc-menu .cs-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  border-radius: var(--radius-sm);
  cursor: pointer;
  font-size: 12px;
  color: var(--text);
}
.grpc-menu .cs-item:hover {
  background: var(--bg-hover);
}
.grpc-menu .cs-item.active {
  background: rgba(255, 122, 61, 0.12);
  color: var(--primary);
}
.grpc-menu .cs-item.disabled {
  cursor: default;
  background: transparent;
}
.grpc-menu-search {
  position: sticky;
  top: 0;
  background: var(--bg-2);
  padding: 4px 4px 6px;
  border-bottom: 1px solid var(--border-2);
  margin: -4px -4px 4px;
  z-index: 1;
}
.grpc-menu-search input {
  width: 100%;
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  color: var(--text);
  padding: 5px 8px;
  font-family: var(--mono);
  font-size: 12px;
}
.grpc-menu-search input:focus {
  outline: none;
  border-color: var(--primary);
}
.grpc-menu .muted {
  color: var(--text-3);
  font-size: 11px;
  flex-shrink: 0;
}
.grpc-tag.sm {
  font-size: 9px;
  padding: 1px 4px;
}
.grpc-stream-tag {
  font-size: 10px;
  font-weight: 600;
  color: var(--yellow);
  background: rgba(245, 185, 66, 0.14);
  padding: 1px 6px;
  border-radius: 999px;
  flex-shrink: 0;
}
.grpc-proto-status {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px 8px;
  font-size: 11.5px;
  color: var(--text-2);
}
.grpc-proto-status .status-dot {
  width: 8px;
  height: 8px;
  border-radius: 999px;
  background: var(--text-3);
}
.grpc-proto-status.muted .status-dot {
  background: var(--text-3);
}
.grpc-proto-status.valid .status-dot {
  background: var(--green);
}
.grpc-proto-status.invalid .status-dot {
  background: var(--red);
}
.grpc-proto-status.valid {
  color: var(--green);
}
.grpc-proto-status.invalid {
  color: var(--red);
}
.grpc-proto-status-text {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.grpc-reparse-btn {
  padding: 2px 8px;
  font-size: 11px;
}

/* === gRPC Proto 设置面板 === */
.grpc-proto {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px;
}
.grpc-proto .form-label {
  margin-top: 4px;
}
.grpc-file-row {
  display: flex;
  gap: 8px;
}
.grpc-import-textarea {
  min-height: 72px;
  font-family: var(--mono);
  font-size: 12px;
  resize: vertical;
}
.grpc-hint {
  margin: 4px 0 0;
  font-size: 12px;
  color: var(--text-2);
}
.grpc-hint code {
  background: var(--bg-2);
  border: 1px solid var(--border);
  border-radius: 3px;
  padding: 1px 4px;
  font-family: var(--mono);
}

/* ---------- Flows ---------- */
.flow-list {
  overflow-y: auto;
  padding: 4px 6px 12px;
}
.flow-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border-radius: 8px;
  cursor: pointer;
  color: var(--text-1);
  border: 1px solid transparent;
  transition: background 0.12s ease, border-color 0.12s ease;
}
.flow-item:hover {
  background: color-mix(in srgb, var(--primary) 8%, transparent);
}
.flow-item.active {
  background: color-mix(in srgb, var(--primary) 14%, transparent);
  border-color: color-mix(in srgb, var(--primary) 30%, transparent);
}
.flow-mode-tag {
  font-size: 10.5px;
  padding: 2px 6px;
  border-radius: 4px;
  border: 1px solid var(--border);
  color: var(--text-2);
  background: var(--bg-2);
  white-space: nowrap;
}
.flow-mode-tag.concurrent {
  color: #f79e6c;
  border-color: color-mix(in srgb, #f79e6c 40%, transparent);
}
.flow-mode-tag.data-driven {
  color: #4fc1ff;
  border-color: color-mix(in srgb, #4fc1ff 40%, transparent);
}
.flow-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
}
.flow-del {
  background: transparent;
  border: none;
  color: var(--text-3);
  cursor: pointer;
  padding: 0 4px;
  font-size: 15px;
  line-height: 1;
}
.flow-del:hover {
  color: #e05b5b;
}
.flow-empty-main {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
}
.flow-empty-inner {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
}
.flow-editor {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.flow-editor-header {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 14px;
  border-bottom: 1px solid var(--border);
  background: var(--bg-1);
}
.flow-name-input {
  flex: 0 1 320px;
  font-size: 15px;
  font-weight: 600;
  border: 1px solid transparent;
  background: transparent;
  color: var(--text-1);
  padding: 6px 8px;
  border-radius: 6px;
  outline: none;
}
.flow-name-input:hover,
.flow-name-input:focus {
  border-color: var(--border);
  background: var(--bg-2);
}
.flow-mode-picker {
  display: flex;
  gap: 4px;
  padding: 3px;
  background: var(--bg-2);
  border-radius: 8px;
  border: 1px solid var(--border);
}
.flow-mode-picker .chip-btn {
  padding: 4px 12px;
  font-size: 12.5px;
  border-radius: 6px;
  border: none;
  background: transparent;
  color: var(--text-2);
  cursor: pointer;
}
.flow-mode-picker .chip-btn.active {
  background: var(--primary);
  color: white;
}
.flow-header-spacer {
  flex: 1;
}
.flow-editor-body {
  flex: 1;
  overflow: auto;
  padding: 14px 18px 18px;
}
.flow-hint {
  font-size: 12.5px;
  color: var(--text-2);
  background: var(--bg-2);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 8px 12px;
  margin-bottom: 14px;
  line-height: 1.6;
}
.flow-hint code {
  background: var(--bg-1);
  padding: 1px 5px;
  border-radius: 4px;
  font-family: var(--mono);
  font-size: 12px;
}
.flow-steps {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.flow-step {
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--bg-2);
  overflow: hidden;
}
.flow-step-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  background: var(--bg-1);
  border-bottom: 1px solid var(--border);
}
.flow-step-idx {
  font-family: var(--mono);
  color: var(--text-3);
  font-size: 12px;
  width: 26px;
}
.flow-step-req {
  flex: 1;
  padding: 6px 8px;
  background: var(--bg-2);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text-1);
  font-size: 12.5px;
}
.flow-step-body {
  padding: 10px 12px;
}
.flow-step-label {
  display: block;
  font-size: 12px;
  color: var(--text-3);
  margin-bottom: 4px;
}
.flow-extract {
  width: 100%;
  min-height: 90px;
  resize: vertical;
  background: var(--bg-1);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 8px 10px;
  font-family: var(--mono);
  font-size: 12.5px;
  color: var(--text-1);
  outline: none;
  box-sizing: border-box;
}
.flow-extract:focus {
  border-color: var(--primary);
}
.flow-simple {
  display: flex;
  flex-direction: column;
  gap: 12px;
  max-width: 720px;
}
.flow-field {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 13px;
}
.flow-field > span {
  width: 90px;
  color: var(--text-2);
}
.flow-field > select,
.flow-field > input {
  flex: 1;
  padding: 6px 10px;
  border-radius: 6px;
  border: 1px solid var(--border);
  background: var(--bg-2);
  color: var(--text-1);
  font-size: 12.5px;
}
.flow-data-toolbar {
  display: flex;
  gap: 8px;
  margin-top: 4px;
}
.flow-data-table-wrap {
  border: 1px solid var(--border);
  border-radius: 8px;
  overflow: auto;
  max-height: 420px;
}
.flow-data-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12.5px;
}
.flow-data-table th,
.flow-data-table td {
  border-bottom: 1px solid var(--border);
  border-right: 1px solid var(--border);
  padding: 4px 6px;
  vertical-align: middle;
}
.flow-data-table th {
  background: var(--bg-2);
  position: sticky;
  top: 0;
  z-index: 1;
}
.flow-data-table th.idx,
.flow-data-table td.idx {
  width: 36px;
  text-align: center;
  color: var(--text-3);
}
.flow-data-table th.empty,
.flow-data-table td.empty {
  text-align: center;
  color: var(--text-3);
  padding: 12px;
}
.flow-data-table input {
  width: 100%;
  border: none;
  background: transparent;
  color: var(--text-1);
  outline: none;
  padding: 2px 4px;
  font-family: inherit;
  font-size: inherit;
}
.flow-data-table input:focus {
  background: color-mix(in srgb, var(--primary) 10%, transparent);
}
.flow-data-table th .col-del,
.flow-data-table td.row-del button {
  background: transparent;
  border: none;
  color: var(--text-3);
  cursor: pointer;
  font-size: 14px;
  padding: 0 4px;
}
.flow-data-table th .col-del:hover,
.flow-data-table td.row-del button:hover {
  color: #e05b5b;
}
.flow-report {
  border-top: 1px solid var(--border);
  background: var(--bg-1);
  max-height: 46%;
  display: flex;
  flex-direction: column;
}
.flow-report-head {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 14px;
  border-bottom: 1px solid var(--border);
  font-size: 13px;
}
.flow-report-head .chip {
  font-size: 12px;
  padding: 2px 8px;
  border-radius: 999px;
  border: 1px solid var(--border);
  color: var(--text-2);
  background: var(--bg-2);
}
.flow-report-head .chip.ok {
  color: #68c968;
  border-color: color-mix(in srgb, #68c968 40%, transparent);
}
.flow-report-head .chip.err {
  color: #e05b5b;
  border-color: color-mix(in srgb, #e05b5b 40%, transparent);
}
.flow-report-body {
  overflow: auto;
}
.flow-report-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12.5px;
}
.flow-report-table th,
.flow-report-table td {
  padding: 6px 10px;
  border-bottom: 1px solid var(--border);
  text-align: left;
  vertical-align: top;
}
.flow-report-table th {
  background: var(--bg-2);
  position: sticky;
  top: 0;
  color: var(--text-2);
  font-weight: 500;
}
.flow-report-table td.idx {
  width: 60px;
  color: var(--text-3);
  font-family: var(--mono);
}
.flow-report-table tr.err td {
  background: color-mix(in srgb, #e05b5b 8%, transparent);
}
.flow-report-table .status {
  font-family: var(--mono);
  font-weight: 600;
}
.flow-report-table .status.ok { color: #68c968; }
.flow-report-table .status.err { color: #e05b5b; }
.flow-report-table .step-name { color: var(--text-1); }
.flow-report-table .step-sub {
  color: var(--text-3);
  font-size: 11.5px;
  margin-top: 2px;
}
.flow-report-table .err-cell {
  color: #e05b5b;
  max-width: 320px;
  word-break: break-word;
}
</style>
