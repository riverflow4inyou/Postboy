// 原型交互
(() => {
  /* ---------- 左侧 rail 切换 ---------- */
  const railBtns = document.querySelectorAll('.rail-btn[data-target]');
  const panels = document.querySelectorAll('.sidebar .panel');
  railBtns.forEach(btn => {
    btn.addEventListener('click', () => {
      railBtns.forEach(b => b.classList.remove('active'));
      btn.classList.add('active');
      panels.forEach(p => p.classList.remove('active'));
      const target = document.getElementById('panel-' + btn.dataset.target);
      if (target) target.classList.add('active');
    });
  });

  /* ---------- 集合树折叠 ---------- */
  document.querySelectorAll('.tree-node.folder > .tree-row').forEach(row => {
    row.addEventListener('click', e => {
      if (e.target.closest('.tree-actions')) return;
      const node = row.parentElement;
      node.classList.toggle('open');
      const caret = row.querySelector('.caret');
      if (caret) caret.textContent = node.classList.contains('open') ? '▾' : '▸';
    });
  });

  /* ---------- 请求项点击高亮 ---------- */
  document.querySelectorAll('.tree-row.request').forEach(row => {
    row.addEventListener('click', () => {
      document.querySelectorAll('.tree-row.request').forEach(r => r.classList.remove('active'));
      row.classList.add('active');
    });
  });

  /* ---------- 参数 Tab ---------- */
  const paramTabs = document.querySelectorAll('.request-pane > .param-tabs > .param-tab');
  const paramContents = document.querySelectorAll('.param-content');
  paramTabs.forEach(tab => {
    tab.addEventListener('click', () => {
      paramTabs.forEach(t => t.classList.remove('active'));
      tab.classList.add('active');
      paramContents.forEach(c => c.classList.remove('active'));
      const target = document.getElementById('tab-' + tab.dataset.tab);
      if (target) target.classList.add('active');
    });
  });

  /* ---------- 标签页 ---------- */
  document.querySelectorAll('.tabs .tab').forEach(tab => {
    if (tab.classList.contains('new')) return;
    tab.addEventListener('click', e => {
      if (e.target.classList.contains('tab-close')) {
        tab.remove();
        return;
      }
      document.querySelectorAll('.tabs .tab').forEach(t => t.classList.remove('active'));
      tab.classList.add('active');
    });
  });

  /* ---------- 自定义下拉（cdrop）---------- */
  const allDrops = document.querySelectorAll('.cdrop');
  allDrops.forEach(drop => {
    const trigger = drop.querySelector('.cdrop-trigger');
    const items = drop.querySelectorAll('.cdrop-item');
    trigger.addEventListener('click', e => {
      e.stopPropagation();
      // 关闭其他
      allDrops.forEach(d => { if (d !== drop) d.classList.remove('open'); });
      drop.classList.toggle('open');
    });
    items.forEach(item => {
      item.addEventListener('click', e => {
        e.stopPropagation();
        const value = item.dataset.value;
        // action 项（如 新建集合）单独处理
        if (item.classList.contains('action')) {
          drop.classList.remove('open');
          if (value === '__new__') {
            // 这里可以打开新建集合 modal，原型暂示意
            alert('新建集合（占位）');
          }
          return;
        }
        // 同步选中态
        drop.querySelectorAll('.cdrop-item').forEach(i => i.classList.remove('selected'));
        item.classList.add('selected');
        drop.dataset.value = value;
        // 同步触发器显示（克隆 item 内除 ✓ 外的内容）
        const newTrigger = item.cloneNode(true);
        // 移除选中类，避免 ::after ✓ 出现在 trigger
        newTrigger.classList.remove('selected');
        trigger.innerHTML = '';
        Array.from(newTrigger.childNodes).forEach(n => trigger.appendChild(n));
        // 补回 caret
        const caret = document.createElement('span');
        caret.className = 'cdrop-caret';
        caret.textContent = '▾';
        trigger.appendChild(caret);
        drop.classList.remove('open');
        // 联动 URL 行方法颜色（如果是方法下拉）
        if (drop.id === 'method-dd') updateMethodTabHint(value);
      });
    });
  });
  // 全局点击关闭下拉
  document.addEventListener('click', () => {
    document.querySelectorAll('.cdrop.open').forEach(d => d.classList.remove('open'));
  });

  function updateMethodTabHint(value) {
    // 更新 tab 中的方法标签颜色（演示效果）
    const activeTab = document.querySelector('.tabs .tab.active .method');
    if (activeTab) {
      activeTab.className = 'method ' + value.toLowerCase();
      activeTab.textContent = value === 'DELETE' ? 'DEL' : value;
    }
  }

  /* ---------- URL 协议自动补全提示 ---------- */
  const urlInput = document.getElementById('url-input');
  const urlHint = document.getElementById('url-hint');
  function refreshUrlHint() {
    const v = urlInput.value.trim();
    // 已经带协议或以变量 {{ 开头则隐藏
    const hasScheme = /^(https?|ws|wss|file):\/\//i.test(v) || v.startsWith('{{');
    const empty = v.length === 0;
    if (hasScheme || empty) {
      urlHint.classList.add('hidden');
    } else {
      urlHint.classList.remove('hidden');
    }
  }
  if (urlInput && urlHint) {
    refreshUrlHint();
    urlInput.addEventListener('input', refreshUrlHint);
    urlInput.addEventListener('blur', refreshUrlHint);
  }

  /* ---------- 垂直分割条：调整 sidebar 宽度 ---------- */
  const vSplitter = document.getElementById('v-splitter');
  const sidebar = document.getElementById('sidebar');
  if (vSplitter && sidebar) {
    let dragging = false, startX = 0, startW = 0;
    vSplitter.addEventListener('mousedown', e => {
      dragging = true;
      startX = e.clientX;
      startW = sidebar.getBoundingClientRect().width;
      vSplitter.classList.add('active');
      document.body.classList.add('resizing');
      e.preventDefault();
    });
    document.addEventListener('mousemove', e => {
      if (!dragging) return;
      const delta = e.clientX - startX;
      let w = startW + delta;
      const min = parseInt(getComputedStyle(sidebar).minWidth) || 200;
      const max = parseInt(getComputedStyle(sidebar).maxWidth) || 520;
      if (w < min) w = min;
      if (w > max) w = max;
      sidebar.style.width = w + 'px';
      sidebar.style.flexBasis = w + 'px';
    });
    document.addEventListener('mouseup', () => {
      if (!dragging) return;
      dragging = false;
      vSplitter.classList.remove('active');
      document.body.classList.remove('resizing');
    });
  }

  /* ---------- 水平分割条：调整 request/response 高度 ---------- */
  const hSplitter = document.getElementById('h-splitter');
  const requestPane = document.getElementById('request-pane');
  const workspace = document.getElementById('workspace');
  if (hSplitter && requestPane && workspace) {
    let dragging = false, startY = 0, startH = 0;
    hSplitter.addEventListener('mousedown', e => {
      dragging = true;
      startY = e.clientY;
      startH = requestPane.getBoundingClientRect().height;
      hSplitter.classList.add('active');
      document.body.classList.add('v-resizing');
      e.preventDefault();
    });
    document.addEventListener('mousemove', e => {
      if (!dragging) return;
      const delta = e.clientY - startY;
      const wsRect = workspace.getBoundingClientRect();
      let h = startH + delta;
      const min = 160;
      const max = wsRect.height - 160; // 给响应区留 160 最小高
      if (h < min) h = min;
      if (h > max) h = max;
      requestPane.style.height = h + 'px';
      requestPane.style.flex = '0 0 ' + h + 'px';
    });
    document.addEventListener('mouseup', () => {
      if (!dragging) return;
      dragging = false;
      hSplitter.classList.remove('active');
      document.body.classList.remove('v-resizing');
    });
  }

  /* ---------- cURL 导入 Modal ---------- */
  const modal = document.getElementById('modal-import');
  const btnImport = document.getElementById('btn-import');
  const btnCloseModal = document.getElementById('modal-close');
  const btnCancelModal = document.getElementById('modal-cancel');
  const curlInput = document.getElementById('curl-input');
  const curlStatus = document.getElementById('curl-status');

  function openModal() { modal.hidden = false; }
  function closeModal() { modal.hidden = true; }
  if (btnImport) btnImport.addEventListener('click', openModal);
  if (btnCloseModal) btnCloseModal.addEventListener('click', closeModal);
  if (btnCancelModal) btnCancelModal.addEventListener('click', closeModal);
  if (modal) modal.addEventListener('click', e => { if (e.target === modal) closeModal(); });

  // cURL 校验（极简）
  function validateCurl() {
    if (!curlInput || !curlStatus) return;
    const v = curlInput.value.trim();
    const ok = /^curl\s+/i.test(v) && /https?:\/\/|[\w.-]+\.[a-z]{2,}/i.test(v);
    curlStatus.classList.toggle('valid', ok);
    curlStatus.classList.toggle('invalid', !ok);
    curlStatus.querySelector('.status-text').textContent = ok
      ? '合法的 cURL · 已识别 1 个请求'
      : '无法识别为合法的 cURL 命令';
  }
  if (curlInput) {
    validateCurl();
    curlInput.addEventListener('input', validateCurl);
  }

  // ESC 关闭
  document.addEventListener('keydown', e => {
    if (e.key === 'Escape' && modal && !modal.hidden) closeModal();
  });
})();
