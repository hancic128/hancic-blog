// 后台全局脚本：CSRF 注入、data-confirm 确认弹窗、抽屉导航（≤768px）、
// 侧栏折叠（桌面端）、导航图标、仪表盘趋势图与设置页键值编辑器、明暗模式切换。
(function () {
  'use strict';

  // ---- 明暗模式切换：localStorage 持久化，Chart.js / Vditor 联动 ----
  var MODE_KEY = 'admin-mode';
  function currentMode() {
    return document.documentElement.getAttribute('data-mode') || 'dark';
  }
  // 暴露给编辑器等独立 IIFE 使用（明暗切换联动）
  window.hancicMode = currentMode;
  function setMode(mode) {
    document.documentElement.setAttribute('data-mode', mode);
    try { localStorage.setItem(MODE_KEY, mode); } catch (e) { /* 忽略 */ }
    // 联动 Chart.js：如果趋势图已创建，更新轴色
    var chart = window._adminChart;
    if (chart) {
      var isLight = mode === 'light';
      var gridColor = isLight ? 'rgba(0,0,0,0.06)' : 'rgba(255,255,255,0.05)';
      var tickColor = isLight ? '#64748B' : '#7C8DB0';
      chart.options.scales.x.grid.color = gridColor;
      chart.options.scales.x.ticks.color = tickColor;
      chart.options.scales.y.grid.color = gridColor;
      chart.options.scales.y.ticks.color = tickColor;
      chart.update('none');
    }
    // 联动 Vditor：切换编辑器主题
    var vditor = window._adminVditor;
    if (vditor && vditor.setTheme) {
      vditor.setTheme(mode === 'light' ? 'classic' : 'dark');
    }
  }
  var modeBtn = document.getElementById('mode-toggle');
  if (modeBtn) {
    modeBtn.addEventListener('click', function () {
      setMode(currentMode() === 'dark' ? 'light' : 'dark');
    });
  }
  // 监听系统偏好变化（仅用户未手动切换时）
  window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', function (e) {
    if (!localStorage.getItem(MODE_KEY)) {
      setMode(e.matches ? 'dark' : 'light');
    }
  });

  // ---- CSRF：读取 meta，注入 POST 表单；fetch 包装自动带 X-CSRF-Token ----
  var csrfMeta = document.querySelector('meta[name="csrf-token"]');
  var csrfToken = csrfMeta ? csrfMeta.getAttribute('content') : '';

  // ---- 确认对话框 / 提示 Toast：替代原生 confirm/alert，样式与后台一致 ----
  (function () {
    'use strict';
    var overlay = null;
    var state = null; // 当前弹窗 { resolve, done, triggerEl }

    function ensure() {
      if (overlay) return;
      overlay = document.createElement('div');
      overlay.className = 'modal-overlay';
      overlay.hidden = true;
      var box = document.createElement('div');
      box.className = 'modal-box';
      box.setAttribute('role', 'dialog');
      box.setAttribute('aria-modal', 'true');
      box.setAttribute('aria-labelledby', 'modal-title');
      var titleEl = document.createElement('h3');
      titleEl.className = 'modal-title';
      titleEl.id = 'modal-title';
      var msgEl = document.createElement('p');
      msgEl.className = 'modal-msg';
      var actionsEl = document.createElement('div');
      actionsEl.className = 'modal-actions';
      box.appendChild(titleEl);
      box.appendChild(msgEl);
      box.appendChild(actionsEl);
      overlay.appendChild(box);
      document.body.appendChild(overlay);
      // 遮罩点击 / Esc 关闭：overlay 为单例，只绑一次
      overlay.addEventListener('click', function (e) {
        if (e.target === overlay) settle(false);
      });
      document.addEventListener('keydown', function (e) {
        if (!overlay.hidden && e.key === 'Escape') settle(false);
      });
    }

    function settle(val) {
      if (!state || state.done) return;
      state.done = true;
      overlay.hidden = true;
      // 仅取消时把焦点还给触发元素；确认后页面即将提交/跳转，不做无谓滚动
      if (!val && state.triggerEl instanceof Element && state.triggerEl.isConnected) {
        state.triggerEl.focus();
      }
      var resolve = state.resolve;
      state = null;
      resolve(val);
    }

    // 返回 Promise<boolean>；确认时 resolve(true)，取消/点遮罩/Esc resolve(false)
    window.hancicConfirm = function (message, triggerEl) {
      ensure();
      if (state) state.resolve(false); // 防御：不应有并发弹窗
      return new Promise(function (resolve) {
        state = { resolve: resolve, done: false, triggerEl: triggerEl };
        var box = overlay.firstChild;
        var titleEl = box.querySelector('.modal-title');
        var msgEl = box.querySelector('.modal-msg');
        var actionsEl = box.querySelector('.modal-actions');
        var danger = /删除|清空|吊销|恢复/.test(message);
        titleEl.textContent = '确认操作';
        msgEl.textContent = message;
        actionsEl.innerHTML = '';
        var cancel = document.createElement('button');
        cancel.type = 'button';
        cancel.className = 'btn';
        cancel.textContent = '取消';
        var ok = document.createElement('button');
        ok.type = 'button';
        ok.className = danger ? 'btn btn-danger' : 'btn btn-primary';
        ok.textContent = '确认';
        actionsEl.appendChild(cancel);
        actionsEl.appendChild(ok);
        ok.addEventListener('click', function () { settle(true); });
        cancel.addEventListener('click', function () { settle(false); });
        overlay.hidden = false;
        ok.focus();
      });
    };

    // 底部轻提示（错误/成功等短暂反馈）
    window.hancicToast = function (message, type) {
      var t = document.createElement('div');
      t.className = 'toast' + (type === 'error' ? ' toast-error' : '');
      t.setAttribute('role', 'status');
      if (type === 'error') {
        t.insertAdjacentHTML('afterbegin',
          '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg>');
      }
      var span = document.createElement('span');
      span.textContent = message;
      t.appendChild(span);
      document.body.appendChild(t);
      setTimeout(function () {
        t.style.transition = 'opacity 0.2s ease';
        t.style.opacity = '0';
        setTimeout(function () {
          if (t.parentNode) t.parentNode.removeChild(t);
        }, 220);
      }, 2600);
    };
  })();

  document.addEventListener('submit', function (e) {
    var form = e.target;
    if (!form || form.method === undefined) return;
    if (form.method.toLowerCase() !== 'post') return;
    // data-confirm 确认弹窗（异步；确认后带标记重提交，避免二次询问）
    if (form.hasAttribute('data-confirm') && !form.dataset.confirmed) {
      e.preventDefault();
      window.hancicConfirm(form.getAttribute('data-confirm'), e.submitter).then(function (ok) {
        if (!ok) return;
        form.dataset.confirmed = '1';
        form.requestSubmit();
      });
      return;
    }
    // 键值编辑器（导航/社交）：提交前把行内容序列化为 JSON 写回隐藏字段
    var eds = form.querySelectorAll('.kv-editor');
    for (var ei = 0; ei < eds.length; ei++) {
      if (eds[ei]._sync) eds[ei]._sync();
    }
    // 未显式携带 csrf 字段的 POST 表单自动补 hidden input
    if (csrfToken && !form.querySelector('input[name="csrf"]')) {
      var input = document.createElement('input');
      input.type = 'hidden';
      input.name = 'csrf';
      input.value = csrfToken;
      form.appendChild(input);
    }
  });

  // 后台 XHR/JSON 请求统一走这里，自动带 CSRF 头。
  window.hancicFetch = function (url, opts) {
    opts = opts || {};
    opts.headers = opts.headers || {};
    if (csrfToken) opts.headers['X-CSRF-Token'] = csrfToken;
    return fetch(url, opts);
  };

  // ---- data-confirm：链接 / 提交按钮点击确认 ----
  // 按钮级 data-confirm（如备份恢复、迁移导入）此前未被拦截，这里一并处理；
  // 表单级 data-confirm 由 submit 事件处理，二者通过 form.dataset.confirmed 互斥。
  document.addEventListener('click', function (e) {
    var el = e.target instanceof Element ? e.target.closest('[data-confirm]') : null;
    if (!el) return;
    if (el.tagName === 'A') {
      e.preventDefault();
      window.hancicConfirm(el.getAttribute('data-confirm'), el).then(function (ok) {
        if (ok) window.location.href = el.href;
      });
      return;
    }
    if (el.tagName === 'BUTTON' && el.type === 'submit') {
      e.preventDefault();
      var form = el.form;
      if (!form) return;
      window.hancicConfirm(el.getAttribute('data-confirm'), el).then(function (ok) {
        if (!ok) return;
        form.dataset.confirmed = '1';
        form.requestSubmit(el);
      });
    }
  });

  // ---- 侧栏导航图标：按 data-icon（url）注入 SVG ----
  var NAV_ICONS = {
    '/admin': '<rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/>',
    '/admin/posts': '<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="16" y1="13" x2="8" y2="13"/><line x1="16" y1="17" x2="8" y2="17"/>',
    '/admin/moments': '<path d="M21 11.5a8.38 8.38 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.38 8.38 0 0 1-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.38 8.38 0 0 1 3.8-.9h.5a8.48 8.48 0 0 1 8 8v.5z"/>',
    '/admin/attachments': '<rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/>',
    '/admin/taxonomy': '<path d="M20.59 13.41l-7.17 7.17a2 2 0 0 1-2.83 0L2 12V2h10l8.59 8.59a2 2 0 0 1 0 2.83z"/><line x1="7" y1="7" x2="7.01" y2="7"/>',
    '/admin/settings': '<line x1="4" y1="21" x2="4" y2="14"/><line x1="4" y1="10" x2="4" y2="3"/><line x1="12" y1="21" x2="12" y2="12"/><line x1="12" y1="8" x2="12" y2="3"/><line x1="20" y1="21" x2="20" y2="16"/><line x1="20" y1="12" x2="20" y2="3"/><line x1="1" y1="14" x2="7" y2="14"/><line x1="9" y1="8" x2="15" y2="8"/><line x1="17" y1="16" x2="23" y2="16"/>',
    '/admin/themes': '<path d="M12 22a10 10 0 1 1 10-10c0 2.2-1.8 4-4 4h-2a3 3 0 0 0-3 3c0 1.5 1 3 3 3z"/><circle cx="7.5" cy="11.5" r="1"/><circle cx="11" cy="7.5" r="1"/><circle cx="15.5" cy="9.5" r="1"/><circle cx="17.5" cy="13.5" r="1"/>',
    '/admin/stats': '<line x1="12" y1="20" x2="12" y2="10"/><line x1="18" y1="20" x2="18" y2="4"/><line x1="6" y1="20" x2="6" y2="16"/>',
    '/admin/tokens': '<path d="M21 2l-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.778 7.778 5.5 5.5 0 0 1 7.777-7.777zm0 0L15.5 7.5m0 0l3 3L22 7l-3-3m-3.5 3.5L19 4"/>',
    '/admin/backup': '<polyline points="21 8 21 21 3 21 3 8"/><rect x="1" y="3" width="22" height="5"/><line x1="10" y1="12" x2="14" y2="12"/>',
    '/admin/migrate': '<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/>',
    '/': '<path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/>',
    'logout': '<path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><polyline points="16 17 21 12 16 7"/><line x1="21" y1="12" x2="9" y2="12"/>'
  };
  document.querySelectorAll('[data-icon]').forEach(function (el) {
    var inner = NAV_ICONS[el.getAttribute('data-icon')] || '';
    if (!inner) return;
    el.innerHTML = '<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' + inner + '</svg>';
  });

  // ---- 侧栏折叠（桌面端）：窄条只显图标，状态存 localStorage ----
  var shell = document.getElementById('admin-shell');
  var collapseBtn = document.getElementById('side-collapse');
  if (shell && collapseBtn) {
    var SIDE_KEY = 'admin-side-collapsed';
    try {
      if (localStorage.getItem(SIDE_KEY) === '1') shell.classList.add('side-collapsed');
    } catch (e) { /* 隐私模式忽略 */ }
    collapseBtn.addEventListener('click', function () {
      var collapsed = shell.classList.toggle('side-collapsed');
      collapseBtn.setAttribute('aria-label', collapsed ? '展开菜单' : '折叠菜单');
      collapseBtn.setAttribute('title', collapsed ? '展开/折叠菜单' : '展开/折叠菜单');
      try { localStorage.setItem(SIDE_KEY, collapsed ? '1' : '0'); } catch (e) { /* 忽略 */ }
    });
  }

  // ---- 侧栏导航分组标题（按 data-group 变化插入）----
  var GROUP_LABELS = { 'content': '内容管理', 'system': '系统', 'dashboard': null, 'link': null };
  var navEl = document.querySelector('.admin-nav');
  if (navEl) {
    var lastGroup = null;
    navEl.querySelectorAll('.admin-nav-item').forEach(function (item) {
      var g = item.getAttribute('data-group');
      if (g === lastGroup) return;
      lastGroup = g;
      var label = GROUP_LABELS[g];
      if (!label) return;
      var heading = document.createElement('div');
      heading.className = 'nav-group';
      heading.textContent = label;
      item.parentNode.insertBefore(heading, item);
    });
  }

  // ---- 设置页键值编辑器（导航菜单 / 社交链接）----
  // 结构：<div class="kv-editor" data-target="site_nav" data-format="nav|social">
  //       行由 JS 管理；提交时 _sync() 序列化 JSON 写回隐藏字段
  document.querySelectorAll('.kv-editor').forEach(function (ed) {
    var target = document.getElementById(ed.dataset.target);
    if (!target) return;
    var format = ed.dataset.format || 'nav';
    var keyPh = format === 'social' ? '平台名，如 github' : '名称，如 首页';
    var urlPh = format === 'social' ? '链接，如 https://github.com/xxx' : '链接，如 /archives';
    function parse() {
      try { var v = JSON.parse(target.value || ''); return v; } catch (e) { return []; }
    }
    function buildRow(keyVal, urlVal) {
      var row = document.createElement('div');
      row.className = 'kv-row';
      var key = document.createElement('input');
      key.className = 'kv-key';
      key.placeholder = keyPh;
      key.setAttribute('aria-label', keyPh);
      key.value = keyVal || '';
      var url = document.createElement('input');
      url.className = 'kv-url';
      url.placeholder = urlPh;
      url.setAttribute('aria-label', urlPh);
      url.value = urlVal || '';
      var del = document.createElement('button');
      del.type = 'button';
      del.className = 'kv-del';
      del.setAttribute('aria-label', '删除这一行');
      del.textContent = '✕';
      del.addEventListener('click', function () { row.remove(); });
      row.appendChild(key);
      row.appendChild(url);
      row.appendChild(del);
      return row;
    }
    var pairs = format === 'social'
      ? Object.keys(parse()).map(function (k) { return [k, parse()[k]]; })
      : parse().map(function (o) { return [o.label || '', o.url || '']; });
    pairs.forEach(function (p) { ed.appendChild(buildRow(p[0], p[1])); });
    if (!ed.querySelector('.kv-row')) ed.appendChild(buildRow('', ''));
    var add = ed.parentNode.querySelector('.kv-add');
    if (add) {
      add.addEventListener('click', function () { ed.appendChild(buildRow('', '')); });
    }
    ed._sync = function () {
      var items = [];
      ed.querySelectorAll('.kv-row').forEach(function (row) {
        var k = row.querySelector('.kv-key').value.trim();
        var u = row.querySelector('.kv-url').value.trim();
        if (!k && !u) return;
        items.push([k, u]);
      });
      if (format === 'social') {
        var obj = {};
        items.forEach(function (p) { if (p[0]) obj[p[0]] = p[1]; });
        target.value = JSON.stringify(obj);
      } else {
        target.value = JSON.stringify(items.map(function (p) {
          return { label: p[0], url: p[1] };
        }));
      }
    };
  });

  // ---- 抽屉导航（≤768px）：body.drawer-open 控制侧边栏滑入 ----
  var toggle = document.getElementById('admin-toggle');

  function drawerOpen() { return document.body.classList.contains('drawer-open'); }
  function openDrawer() { document.body.classList.add('drawer-open'); }
  function closeDrawer() { document.body.classList.remove('drawer-open'); }

  if (toggle) {
    toggle.addEventListener('click', function () {
      if (drawerOpen()) { closeDrawer(); } else { openDrawer(); }
    });
  }
  // 点击侧边栏/按钮以外的区域关闭抽屉
  document.addEventListener('click', function (e) {
    if (!drawerOpen()) return;
    if (e.target.closest('#admin-side') || e.target.closest('#admin-toggle')) return;
    closeDrawer();
  });
  // 点击导航项后自动收起（移动端体验）
  var side = document.getElementById('admin-side');
  if (side) {
    side.querySelectorAll('a').forEach(function (a) {
      a.addEventListener('click', closeDrawer);
    });
  }

  // ---- 仪表盘趋势图（Chart.js）----
  document.addEventListener('DOMContentLoaded', function () {
    var canvas = document.getElementById('trend');
    if (!canvas || !window.Chart || !window.chartData) return;
    var isLight = currentMode() === 'light';
    var accentColor = isLight ? '#16A34A' : '#22C55E';
    var gridColor = isLight ? 'rgba(0,0,0,0.06)' : 'rgba(255,255,255,0.05)';
    var tickColor = isLight ? '#64748B' : '#7C8DB0';
    window._adminChart = new window.Chart(canvas, {
      type: 'line',
      data: {
        labels: window.chartData.labels,
        datasets: [{
          label: '阅读量',
          data: window.chartData.data,
          borderColor: accentColor,
          backgroundColor: isLight ? 'rgba(22, 163, 74, 0.08)' : 'rgba(34, 197, 94, 0.10)',
          fill: true,
          tension: 0.3,
          pointRadius: 2,
          pointBackgroundColor: accentColor,
          borderWidth: 2
        }]
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        plugins: { legend: { display: false } },
        scales: {
          x: { grid: { color: gridColor }, ticks: { color: tickColor } },
          y: { beginAtZero: true, ticks: { precision: 0, color: tickColor }, grid: { color: gridColor } }
        }
      }
    });
  });
})();

// ---- 文章编辑器（Vditor IR）与自动保存 ----
(function () {
  'use strict';

  var editorEl = document.getElementById('editor');
  // vditor.min.js 在 admin.js 之后加载（layout.html 的 block scripts 在其后），
  // 故等到 DOMContentLoaded（此时所有经典脚本已执行完毕）再初始化编辑器。
  if (!editorEl) return;
  if (window.Vditor) {
    initEditor();
  } else {
    document.addEventListener('DOMContentLoaded', initEditor);
  }

  // 新建页本地草稿的 localStorage key（Vditor cache 的 id，恢复/清除共用）。
  var DRAFT_KEY = 'vditor-draft';

  function initEditor() {
    var form = document.getElementById('post-form');
    var statusInput = document.getElementById('post-status');
    var statusEl = document.getElementById('save-status');
    var editor = null;
    var lastSaved = null;
    // 新建页（_post 无 id）：无服务端 autosave，改走 Vditor 本地草稿（I1）
    var isNewPost = !window._post || !window._post.id;

    // Vditor 上传处理器：files → POST /api/uploads（hancicFetch 自动带 CSRF 头）。
    // Vditor 3.x 的 upload.handler 契约要求处理器自行把结果插入编辑器：
    // 返回 undefined 表示成功，返回字符串会被当作错误提示展示。
    // res 非 2xx 直接抛错（I2：上传失败不得静默当作成功），错误消息带
    // 服务端返回/401 场景提示。
    window.vditorUpload = function (files) {
      var data = new FormData();
      files.forEach(function (file) { data.append('files', file); });
      return window.hancicFetch('/api/uploads', { method: 'POST', body: data })
        .then(function (res) {
          if (res.ok) return res.json();
          return res.json().then(function (body) {
            var serverMsg = body && body.error && body.error.message;
            var msg = serverMsg || ('上传失败（HTTP ' + res.status + '）');
            if (res.status === 401 || res.status === 403) {
              msg = '登录已过期，请刷新页面重新登录';
            }
            throw new Error(msg);
          }).catch(function (e) {
            if (e instanceof Error && e.message) throw e;
            throw new Error('上传失败（HTTP ' + res.status + '）');
          });
        })
        .then(function (json) {
          (json.data || []).forEach(function (att) {
            editor.insertValue('![' + att.orig_name + '](/uploads/' + att.path + ')\n');
          });
          return undefined;
        })
        .catch(function (err) {
          return err && err.message ? err.message : '上传失败，请重试';
        });
    };

    function currentContent() {
      return editor ? editor.getValue() : '';
    }

    function contentChanged() {
      return lastSaved === null || currentContent() !== lastSaved;
    }

    function showSaved() {
      if (!statusEl) return;
      var now = new Date();
      var hh = ('0' + now.getHours()).slice(-2);
      var mm = ('0' + now.getMinutes()).slice(-2);
      statusEl.textContent = '已保存 ' + hh + ':' + mm;
    }

    // 统一自动保存入口（I1）：keepalive 保证 pagehide/关页时请求随页面提交；
    // 请求体超 keepalive 64KB 上限（fetch 会同步抛 TypeError）时回退同步 XHR
    // ——页面卸载场景也能把请求发完，杜绝大正文关页丢稿。
    function autosaveNow() {
      if (!editor || !window._post || !window._post.id) return;
      if (!contentChanged()) return;
      var md = currentContent();
      var url = '/admin/posts/' + window._post.id + '/autosave';
      if (md.length > 65536) {
        saveSyncXhr(url, md);
      } else {
        saveWithKeepalive(url, md);
      }
    }

    function saveWithKeepalive(url, md) {
      var data = new FormData();
      data.append('content_md', md);
      try {
        window.hancicFetch(url, { method: 'POST', body: data, keepalive: true })
          .then(function (res) { return res.ok ? res.json() : Promise.reject(res); })
          .then(function () {
            lastSaved = md;
            showSaved();
          })
          .catch(function () { /* 静默失败：下一次间隔/pagehide/blur 重试 */ });
      } catch (e) {
        // 同步抛错 = 超出 keepalive 上限，回退同步 XHR
        saveSyncXhr(url, md);
      }
    }

    function saveSyncXhr(url, md) {
      var token = csrfToken();
      var data = new URLSearchParams();
      data.append('content_md', md);
      data.append('csrf', token);
      var xhr = new XMLHttpRequest();
      xhr.open('POST', url, false); // 同步：pagehide 场景必须发完才返回
      if (token) xhr.setRequestHeader('X-CSRF-Token', token);
      xhr.setRequestHeader('Content-Type', 'application/x-www-form-urlencoded');
      try {
        xhr.send(data.toString());
        if (xhr.status >= 200 && xhr.status < 300) {
          lastSaved = md;
          showSaved();
        }
      } catch (e) { /* 同步 XHR 失败只能静默 */ }
    }

    // 存草稿 / 发布：点击的按钮 data-action 写入隐藏 status；正文 content_md
    // 由 Vditor 取值补进隐藏字段（编辑器是 div，不会随表单自动提交）。
    if (form) {
      form.addEventListener('submit', function (e) {
        // 发布/存草稿确认提示（异步；确认后带标记重提交，避免二次询问）
        var sb = e.submitter;
        var action = sb && sb.dataset ? sb.dataset.action : null;
        if (action && !form.dataset.confirmed) {
          e.preventDefault();
          var label = action === 'published' ? '发布' : '保存草稿';
          window.hancicConfirm('确认' + label + '吗？', sb).then(function (ok) {
            if (!ok) return;
            form.dataset.confirmed = '1';
            statusInput.value = action;
            form.requestSubmit(sb);
          });
          return;
        }
        var md = document.createElement('input');
        md.type = 'hidden';
        md.name = 'content_md';
        md.value = currentContent();
        form.appendChild(md);
      });
    }

    // 初始化 Vditor：IR 模式，内容取自容器 data-content（页面已 tera 转义）。
    // cdn 指向本地 /static/vendor/vditor（i18n/lute/icons 已随仓库 assets 发布，
    // 见 scripts/fetch-assets.sh），避免运行时外网依赖（M120）。
    // 新建页启用 Vditor 内置 cache（写入 localStorage 草稿并恢复，I1）；
    // 编辑页关闭 cache（服务端内容为准，autosave 负责落库）。
    editor = new window.Vditor('editor', {
      mode: 'ir',
      cdn: '/static/vendor/vditor',
      theme: window.hancicMode() === 'light' ? 'classic' : 'dark',
      cache: isNewPost ? { enable: true, id: DRAFT_KEY } : false,
      height: 460,
      value: editorEl.getAttribute('data-content') || '',
      after: function () {
        lastSaved = editor.getValue();
        // 保存编辑器实例供明暗切换联动
        window._adminVditor = editor;
        // 已保存文章（有 id）进入编辑页时清掉新建页草稿，防止误恢复（I1）
        if (!isNewPost && window.localStorage) {
          window.localStorage.removeItem(DRAFT_KEY);
        }
      },
      blur: function () { autosaveNow(); },
      upload: { handler: window.vditorUpload }
    });

    // 自动保存：30s 轮询 + blur（编辑器失焦）+ pagehide 兜底（仅内容变化时发请求）
    setInterval(autosaveNow, 30000);
    window.addEventListener('pagehide', autosaveNow);
  }

  // CSRF token：编辑器 IIFE 无法访问外层闭包变量，直接读 meta（sync XHR 需要）。
  function csrfToken() {
    var meta = document.querySelector('meta[name="csrf-token"]');
    return meta ? meta.getAttribute('content') : '';
  }
})();


// ---- 文章编辑页：标签 chips（回车/逗号添加，datalist 选择已有，点击删除）----
(function () {
  'use strict';
  var editor = document.getElementById('tag-editor');
  var hidden = document.getElementById('tags-hidden');
  var input = document.getElementById('post-tags');
  if (!editor || !hidden || !input) return;
  var chips = document.getElementById('tag-chips');

  function currentTags() {
    return (hidden.value || '').split(',').map(function (s) { return s.trim(); }).filter(Boolean);
  }
  function render() {
    chips.innerHTML = '';
    currentTags().forEach(function (tag) {
      var chip = document.createElement('span');
      chip.className = 'tag-chip';
      chip.textContent = tag;
      chip.title = '点击删除';
      chip.addEventListener('click', function () {
        var next = currentTags().filter(function (t) { return t !== tag; });
        hidden.value = next.join(',');
        render();
      });
      chips.appendChild(chip);
    });
  }
  function addTag(raw) {
    var tag = (raw || '').trim().replace(/,$/, '');
    if (!tag) return;
    var cur = currentTags();
    if (cur.indexOf(tag) === -1) {
      cur.push(tag);
      hidden.value = cur.join(',');
      render();
    }
  }
  render();
  input.addEventListener('keydown', function (e) {
    if (e.key === 'Enter' || e.key === ',') {
      e.preventDefault();
      addTag(input.value);
      input.value = '';
    } else if (e.key === 'Backspace' && !input.value && currentTags().length) {
      var cur = currentTags();
      cur.pop();
      hidden.value = cur.join(',');
      render();
    }
  });
  input.addEventListener('change', function () {
    addTag(input.value);
    input.value = '';
  });
})();

// ---- 自定义下拉组件：替代原生 <select>（消除浏览器默认外观）----
// 结构：<div class="cs" data-select="#real-select-id"><button class="cs-btn">…</button>
//       <ul class="cs-menu"><li data-val="…">…</li></ul></div>，JS 自动从 select 生成。
// 按钮内容固定为 [label, chevron]；label 每次用 textContent 覆盖更新，
// 避免历史实现里每次选中都 append 新文本节点导致「杂思杂思」式重复。
(function () {
  'use strict';

  function buildMenu(sel, wrap) {
    var menu = document.createElement('ul');
    menu.className = 'cs-menu';
    menu.hidden = true;
    Array.prototype.forEach.call(sel.options, function (opt) {
      var li = document.createElement('li');
      li.className = 'cs-option';
      li.dataset.val = opt.value;
      li.textContent = opt.textContent;
      if (opt.selected) li.classList.add('selected');
      menu.appendChild(li);
    });
    wrap.appendChild(menu);
    return menu;
  }

  function syncSelected(sel, menu, labelEl) {
    labelEl.textContent = sel.options[sel.selectedIndex]
      ? sel.options[sel.selectedIndex].textContent
      : '';
    Array.prototype.forEach.call(menu.querySelectorAll('.cs-option'), function (li) {
      li.classList.toggle('selected', li.dataset.val === sel.value);
    });
  }

  document.querySelectorAll('.cs').forEach(function (wrap) {
    var sel = document.querySelector(wrap.dataset.select);
    if (!sel) return;
    var btn = wrap.querySelector('.cs-btn');
    if (!btn) return;
    // 初始化：清空占位内容，固定结构 = 选中标签 + 箭头图标
    btn.innerHTML = '';
    var labelEl = document.createElement('span');
    labelEl.className = 'cs-label';
    btn.appendChild(labelEl);
    btn.insertAdjacentHTML('beforeend',
      '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="6 9 12 15 18 9"/></svg>');
    var menu = buildMenu(sel, wrap);
    syncSelected(sel, menu, labelEl);

    btn.addEventListener('click', function (e) {
      e.stopPropagation();
      var open = !menu.hidden;
      document.querySelectorAll('.cs-menu').forEach(function (m) { m.hidden = true; });
      menu.hidden = open;
      btn.setAttribute('aria-expanded', String(!open));
    });
    menu.addEventListener('click', function (e) {
      var li = e.target.closest('.cs-option');
      if (!li) return;
      sel.value = li.dataset.val;
      sel.dispatchEvent(new Event('change', { bubbles: true }));
      menu.hidden = true;
      btn.setAttribute('aria-expanded', 'false');
      syncSelected(sel, menu, labelEl);
    });
    // 点击外部关闭
    document.addEventListener('click', function () {
      if (!menu.hidden) {
        menu.hidden = true;
        btn.setAttribute('aria-expanded', 'false');
      }
    });
    // 键盘：Esc 关闭
    btn.addEventListener('keydown', function (e) {
      if (e.key === 'Escape' && !menu.hidden) {
        menu.hidden = true;
        btn.setAttribute('aria-expanded', 'false');
      }
    });
  });
})();

// ---- 筛选栏：自定义下拉选择后自动应用过滤（无需再点「筛选」按钮）----
// 自定义下拉在 li 点击时对原生 select 派发 change（bubbles），这里统一监听提交表单。
(function () {
  'use strict';
  document.querySelectorAll('.filter-bar select').forEach(function (sel) {
    sel.addEventListener('change', function () {
      var form = sel.closest('form');
      if (form) form.submit();
    });
  });
})();

// ---- 通用表格排序：小型数据表（无服务端分页）点击表头按列排序 ----
(function () {
  'use strict';
  document.querySelectorAll('table.data-sortable').forEach(function (table) {
    var thead = table.querySelector('thead');
    if (!thead) return;
    Array.prototype.forEach.call(thead.querySelectorAll('th'), function (th, colIdx) {
      if (th.classList.contains('col-ops') || !th.textContent.trim()) return;
      th.style.cursor = 'pointer';
      th.addEventListener('click', function () {
        var asc = th.dataset.dir !== 'asc';
        th.dataset.dir = asc ? 'asc' : 'desc';
        Array.prototype.forEach.call(thead.querySelectorAll('th'), function (t) {
          if (t !== th) delete t.dataset.dir;
        });
        var rows = Array.prototype.slice.call(table.tBodies[0].rows);
        rows.sort(function (a, b) {
          var av = a.cells[colIdx].textContent.trim();
          var bv = b.cells[colIdx].textContent.trim();
          var na = parseFloat(av), nb = parseFloat(bv);
          var cmp = (!isNaN(na) && !isNaN(nb)) ? na - nb : av.localeCompare(bv, 'zh');
          return asc ? cmp : -cmp;
        });
        rows.forEach(function (row) { table.tBodies[0].appendChild(row); });
        // 更新箭头
        Array.prototype.forEach.call(thead.querySelectorAll('th'), function (t) {
          var arrow = t.querySelector('.sort-arrow');
          if (arrow) arrow.remove();
        });
        var arrow = document.createElement('span');
        arrow.className = 'sort-arrow';
        arrow.textContent = asc ? '↑' : '↓';
        th.appendChild(arrow);
      });
    });
  });
})();
