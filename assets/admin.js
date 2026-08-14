// 后台全局脚本：CSRF 注入、data-confirm 确认弹窗、抽屉导航（≤768px）、
// 侧栏折叠（桌面端）、导航图标、仪表盘趋势图与设置页键值编辑器、明暗模式切换。
(function () {
  'use strict';

  // ---- 明暗模式切换：localStorage 持久化，Chart.js 联动 ----
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
    // milkdown 编辑器明暗随 CSS 变量（[data-mode]）自动切换，无需 JS 联动
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

  // ---- 主题配色切换：与前台博客一致的内置 5 色（localStorage 持久化） ----
  var ACCENT_KEY = 'admin-accent';
  var ACCENTS = ['pink', 'blue', 'green', 'purple', 'orange'];
  function currentAccent() {
    return document.documentElement.getAttribute('data-accent') || 'green';
  }
  function markCurrentSwatch() {
    var cur = currentAccent();
    document.querySelectorAll('.accent-swatch').forEach(function (s) {
      s.classList.toggle('current', s.getAttribute('data-accent') === cur);
    });
  }
  function applyAccent(accent) {
    document.documentElement.setAttribute('data-accent', accent);
    try { localStorage.setItem(ACCENT_KEY, accent); } catch (e) { /* 忽略 */ }
    markCurrentSwatch();
  }
  var savedAccent;
  try { savedAccent = localStorage.getItem(ACCENT_KEY); } catch (e) { savedAccent = null; }
  if (savedAccent && ACCENTS.indexOf(savedAccent) !== -1) {
    document.documentElement.setAttribute('data-accent', savedAccent);
  }
  markCurrentSwatch();
  var accentToggle = document.getElementById('accent-toggle');
  var accentPanel = document.getElementById('accent-panel');
  if (accentToggle && accentPanel) {
    accentToggle.addEventListener('click', function () {
      var willShow = accentPanel.hidden;
      accentPanel.hidden = !willShow;
      accentToggle.setAttribute('aria-expanded', willShow ? 'true' : 'false');
    });
    accentPanel.addEventListener('click', function (e) {
      var swatch = e.target.closest('.accent-swatch');
      if (!swatch) return;
      applyAccent(swatch.getAttribute('data-accent'));
      accentPanel.hidden = true;
      accentToggle.setAttribute('aria-expanded', 'false');
    });
    // 点击页面其他区域关闭色板
    document.addEventListener('click', function (e) {
      if (!e.target.closest('.accent-wrap') && !accentPanel.hidden) {
        accentPanel.hidden = true;
        accentToggle.setAttribute('aria-expanded', 'false');
      }
    });
  }

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

    // 返回 Promise<string|null>；确认 resolve(输入值)，取消/遮罩/Esc resolve(null)。
    // 遮罩/Esc 走 confirm 的共享监听（settle(false)），这里把 false 归一为 null。
    window.hancicPrompt = function (message, defaultValue) {
      ensure();
      if (state) state.resolve(null);
      return new Promise(function (resolve) {
        state = {
          resolve: function (v) { resolve(v === false ? null : v); },
          done: false,
          triggerEl: null
        };
        var box = overlay.firstChild;
        var titleEl = box.querySelector('.modal-title');
        var msgEl = box.querySelector('.modal-msg');
        var actionsEl = box.querySelector('.modal-actions');
        titleEl.textContent = message;
        msgEl.innerHTML = '';
        var input = document.createElement('input');
        input.type = 'text';
        input.className = 'modal-input';
        input.value = defaultValue || '';
        msgEl.appendChild(input);
        actionsEl.innerHTML = '';
        var cancel = document.createElement('button');
        cancel.type = 'button';
        cancel.className = 'btn';
        cancel.textContent = '取消';
        var ok = document.createElement('button');
        ok.type = 'button';
        ok.className = 'btn btn-primary';
        ok.textContent = '确定';
        actionsEl.appendChild(cancel);
        actionsEl.appendChild(ok);
        var done = false;
        function finish(val) {
          if (done) return;
          done = true;
          overlay.hidden = true;
          state = null;
          resolve(val === false ? null : val);
        }
        ok.addEventListener('click', function () { finish(input.value.trim()); });
        cancel.addEventListener('click', function () { finish(null); });
        input.addEventListener('keydown', function (e) {
          if (e.key === 'Enter') { e.preventDefault(); finish(input.value.trim()); }
        });
        overlay.hidden = false;
        input.focus();
        input.select();
      });
    };

    // 底部轻提示（错误/成功等短暂反馈）
    window.hancicToast = function (message, type) {      var t = document.createElement('div');
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
    '/admin/system': '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"/>',
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

  // ---- 设置页键值编辑器（导航菜单 / 友情链接 / 社交链接 / 社交图标）----
  // 结构：<div class="kv-editor" data-target="..." data-format="nav|friend|social|logo">
  // 行由 JS 管理；提交时 _sync() 序列化 JSON 写回隐藏字段。
  //   nav    导航菜单：[类型(首页|文章|说说|页面|链接), 名称, 路径]（非链接类型路径预设/隐藏）
  //   friend 友情链接：[名称, 链接]，无类型
  //   social 社交链接 / 二维码：[平台名, 链接]
  //   logo   社交图标：[平台名, 图片URL]（上传 / 附件库选择）
  // nav/friend 支持拖拽排序（行首手柄）。
  var KV_TYPES = [
    ['home', '首页', '/'],
    ['articles', '文章', '/archives'],
    ['moments', '说说', '/moments'],
    ['pages', '页面', ''],
    ['link', '链接', '']
  ];
  function kvTypeDefaultUrl(t) {
    for (var i = 0; i < KV_TYPES.length; i++) {
      if (KV_TYPES[i][0] === t) return KV_TYPES[i][2];
    }
    return '';
  }
  // 图片选择：弹窗（上传 / 附件库网格），选中后回调 URL
  function kvBuildMediaPicker(row, value, onChange) {
    var wrap = document.createElement('div');
    wrap.className = 'kv-media';
    var thumb = document.createElement('img');
    thumb.className = 'kv-media-thumb';
    thumb.alt = '';
    var pickBtn = document.createElement('button');
    pickBtn.type = 'button';
    pickBtn.className = 'btn btn-sm';
    pickBtn.textContent = '选择图片';
    var clearBtn = document.createElement('button');
    clearBtn.type = 'button';
    clearBtn.className = 'kv-media-clear';
    clearBtn.textContent = '×';
    clearBtn.setAttribute('aria-label', '清除图片');
    function render() {
      if (value) {
        thumb.src = value;
        thumb.hidden = false;
        clearBtn.hidden = false;
      } else {
        thumb.hidden = true;
        clearBtn.hidden = true;
      }
    }
    pickBtn.addEventListener('click', function () {
      window.hancicPickImage(function (url) { value = url; render(); onChange(value); });
    });
    clearBtn.addEventListener('click', function () { value = ''; render(); onChange(value); });
    render();
    wrap.appendChild(thumb);
    wrap.appendChild(pickBtn);
    wrap.appendChild(clearBtn);
    return wrap;
  }
  // 页面类型导航：搜索选择具体页面（下拉只列出 type=page 的独立页，支持关键词过滤）
  // 数据懒读取 window.__ALL_POSTS__（admin.js 先于模板数据脚本执行；后端已只注入页面类型）
  function kvBuildPagePick(currentUrl, onChange) {
    var wrap = document.createElement('div');
    wrap.className = 'kv-page-pick';
    function allPosts() { return (window.__ALL_POSTS__ || []).filter(function (p) { return p.type === 'page'; }); }
    function pathOf(p) { return '/page/' + p.slug; }
    function findTitle(url) {
      var posts = allPosts();
      for (var i = 0; i < posts.length; i++) {
        if (pathOf(posts[i]) === url) return posts[i].title;
      }
      return '';
    }
    var btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'kv-page-btn';
    function refreshBtn() {
      // 已选页面：显示标题；找不到匹配（如旧数据存了 /post/ 路径）时回退显示路径本身
      btn.textContent = currentUrl ? (findTitle(currentUrl) || currentUrl) : '搜索选择页面…';
    }
    refreshBtn();
    // window.__ALL_POSTS__ 在模板 scripts 块赋值（晚于 admin.js 初始化），
    // 等解析完再重算一次，确保已选页面的标题正确回显
    setTimeout(refreshBtn, 0);
    btn.setAttribute('aria-haspopup', 'listbox');
    btn.setAttribute('aria-expanded', 'false');
    var panel = document.createElement('div');
    panel.className = 'kv-page-panel';
    panel.hidden = true;
    var search = document.createElement('input');
    search.className = 'kv-page-search';
    search.placeholder = '输入关键词搜索…';
    search.setAttribute('aria-label', '搜索页面');
    var list = document.createElement('ul');
    list.className = 'kv-page-list';
    function render(filter) {
      list.textContent = '';
      var posts = allPosts();
      var matched = 0;
      posts.forEach(function (p) {
        if (filter && p.title.indexOf(filter) === -1 && p.slug.indexOf(filter) === -1) return;
        matched++;
        var li = document.createElement('li');
        li.textContent = p.title;
        li.setAttribute('role', 'option');
        li.addEventListener('click', function () {
          onChange(pathOf(p), p.title);
          panel.hidden = true;
          btn.setAttribute('aria-expanded', 'false');
        });
        list.appendChild(li);
      });
      if (!matched) {
        var empty = document.createElement('li');
        empty.className = 'kv-page-empty';
        empty.textContent = posts.length ? '无匹配页面' : '暂无页面';
        list.appendChild(empty);
      }
    }
    render('');
    btn.addEventListener('click', function (e) {
      e.stopPropagation();
      var willShow = panel.hidden;
      document.querySelectorAll('.kv-page-panel').forEach(function (x) { x.hidden = true; });
      panel.hidden = !willShow;
      btn.setAttribute('aria-expanded', String(!willShow));
      if (willShow) { search.value = ''; render(''); search.focus(); }
    });
    search.addEventListener('input', function () { render(search.value.trim()); });
    document.addEventListener('click', function (e) {
      if (!wrap.contains(e.target)) { panel.hidden = true; btn.setAttribute('aria-expanded', 'false'); }
    });
    panel.appendChild(search);
    panel.appendChild(list);
    wrap.appendChild(btn);
    wrap.appendChild(panel);
    return wrap;
  }
  function kvBuildRow(ed, format, keyVal, urlVal, typeVal) {
    var row = document.createElement('div');
    row.className = 'kv-row';
    // 内置导航项（首页/文章/说说）：固定存在、类型锁定、不可删除，仅可改名与排序
    var builtin = format === 'nav' && ['home', 'articles', 'moments'].indexOf(typeVal) !== -1;
    // 所有列表型编辑器均支持拖拽排序（导航/友情链接/社交链接/社交图标）
    var drag = document.createElement('span');
    drag.className = 'kv-drag';
    drag.title = '拖拽排序';
    drag.setAttribute('aria-label', '拖拽排序');
    drag.innerHTML = '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="9" cy="6" r="1"/><circle cx="9" cy="12" r="1"/><circle cx="9" cy="18" r="1"/><circle cx="15" cy="6" r="1"/><circle cx="15" cy="12" r="1"/><circle cx="15" cy="18" r="1"/></svg>';
    row.appendChild(drag);
    if (format === 'nav') {
      var typeSel = document.createElement('select');
      typeSel.className = 'kv-type';
      typeSel.setAttribute('aria-label', '导航类型');
      KV_TYPES.forEach(function (t) {
        // 内置项仅展示自身类型；其他行只能选「页面」或「链接」
        if (builtin ? t[0] === typeVal : (t[0] === 'pages' || t[0] === 'link')) {
          var opt = document.createElement('option');
          opt.value = t[0];
          opt.textContent = t[1];
          opt.selected = (t[0] === typeVal) || (!typeVal && t[0] === 'link');
          typeSel.appendChild(opt);
        }
      });
      if (builtin) typeSel.disabled = true;
      row.appendChild(typeSel);
    }
    var keyPlaceholders = {
      nav: '如 关于，菜单栏显示的名称',
      friend: '如 Hancic-blog官网',
      social: '如 Github/CSDN/掘金等',
      logo: '如 微信/抖音/小红书等'
    };
    // 内置导航项名称按类型提示（首页/文章/说说）
    var navKeyPh = {
      home: '如 首页，菜单栏显示的名称',
      articles: '如 文章，菜单栏显示的名称',
      moments: '如 说说，菜单栏显示的名称',
      pages: '如 留言板，菜单栏显示的名称',
      link: '如 关于，菜单栏显示的名称'
    };
    var key = document.createElement('input');
    key.className = 'kv-key';
    key.placeholder = format === 'nav' ? (navKeyPh[typeVal] || navKeyPh.link) : (keyPlaceholders[format] || '名称');
    key.setAttribute('aria-label', key.placeholder);
    key.value = keyVal || '';
    row.appendChild(key);
    var url;
    if (format === 'logo') {
      // 图片选择列（替换文本 URL 输入）
      var urlHidden = document.createElement('input');
      urlHidden.type = 'hidden';
      urlHidden.className = 'kv-url';
      urlHidden.value = urlVal || '';
      url = urlHidden;
      var media = kvBuildMediaPicker(row, urlVal || '', function (v) { urlHidden.value = v; });
      row.appendChild(urlHidden);
      row.appendChild(media);
    } else if (format === 'nav' && typeVal === 'pages') {
      // 页面类型：搜索选择具体文章（隐藏 url 存 /post|page/slug）
      var urlHidden2 = document.createElement('input');
      urlHidden2.type = 'hidden';
      urlHidden2.className = 'kv-url';
      urlHidden2.value = urlVal || '';
      url = urlHidden2;
      var pick = kvBuildPagePick(urlVal || '', function (u, title) {
        urlHidden2.value = u;
        pick.querySelector('.kv-page-btn').textContent = title;
      });
      row.appendChild(urlHidden2);
      row.appendChild(pick);
    } else {
      url = document.createElement('input');
      url.className = 'kv-url';
      var urlPlaceholders = {
        nav: '如 https://hancic-blog.site',
        friend: '如 https://hancic-blog.org',
        social: '链接，如 https://github.com/xxx'
      };
      url.placeholder = urlPlaceholders[format] || '';
      url.setAttribute('aria-label', url.placeholder);
      url.value = urlVal || '';
      if (format === 'nav' && typeVal !== 'link') {
        url.style.display = 'none';
        if (!urlVal) url.value = kvTypeDefaultUrl(typeVal || 'link');
      }
      row.appendChild(url);
    }
    if (format === 'nav' && !builtin) {
      typeSel.addEventListener('change', function () {
        // 类型切换后重建该行（link→文本输入；pages→搜索选择；内置→预设路径）
        var newRow = kvBuildRow(ed, format, key.value.trim(), url.value.trim(), typeSel.value);
        row.replaceWith(newRow);
      });
    }
    if (!builtin) {
      var del = document.createElement('button');
      del.type = 'button';
      del.className = 'kv-del';
      del.setAttribute('aria-label', '删除这一行');
      del.textContent = '✕';
      del.addEventListener('click', function () { row.remove(); });
      row.appendChild(del);
    }
    return row;
  }
  // 行拖拽排序（nav/friend）：按住手柄 → ghost 跟手 → 松手落位
  function kvEnableSort(ed) {
    var dragEl = null;
    var ghost = null;
    var offX = 0, offY = 0;
    ed.addEventListener('mousedown', function (e) {
      var handle = e.target.closest('.kv-drag');
      if (!handle) return;
      if (e.button !== 0) return;
      e.preventDefault();
      var row = handle.closest('.kv-row');
      var rect = row.getBoundingClientRect();
      offX = e.clientX - rect.left;
      offY = e.clientY - rect.top;
      ghost = row.cloneNode(true);
      ghost.classList.add('kv-ghost');
      ghost.style.width = rect.width + 'px';
      document.body.appendChild(ghost);
      dragEl = row;
      row.classList.add('kv-dragging');
      moveGhost(e.clientX, e.clientY);
      function moveGhost(x, y) {
        ghost.style.left = (x - offX) + 'px';
        ghost.style.top = (y - offY) + 'px';
      }
      function clearTargets() {
        ed.querySelectorAll('.kv-row').forEach(function (r) { r.classList.remove('kv-drop-before', 'kv-drop-after'); });
      }
      function onMove(ev) {
        if (!dragEl) return;
        moveGhost(ev.clientX, ev.clientY);
        var target = document.elementFromPoint(ev.clientX, ev.clientY);
        var t = target && target.closest('.kv-row');
        clearTargets();
        if (t && t !== dragEl) {
          var r = t.getBoundingClientRect();
          t.classList.add(ev.clientY > r.top + r.height / 2 ? 'kv-drop-after' : 'kv-drop-before');
        }
      }
      function onUp(ev) {
        document.removeEventListener('mousemove', onMove);
        document.removeEventListener('mouseup', onUp);
        if (!dragEl) return;
        var target = document.elementFromPoint(ev.clientX, ev.clientY);
        var t = target && target.closest('.kv-row');
        if (t && t !== dragEl) {
          var r = t.getBoundingClientRect();
          if (ev.clientY > r.top + r.height / 2) t.after(dragEl);
          else t.before(dragEl);
        }
        dragEl.classList.remove('kv-dragging');
        dragEl = null;
        if (ghost) { ghost.remove(); ghost = null; }
        clearTargets();
      }
      document.addEventListener('mousemove', onMove);
      document.addEventListener('mouseup', onUp);
    });
  }
  document.querySelectorAll('.kv-editor').forEach(function (ed) {
    var target = document.getElementById(ed.dataset.target);
    if (!target) return;
    var format = ed.dataset.format || 'nav';
    function parse() {
      try { var v = JSON.parse(target.value || ''); return v; } catch (e) { return []; }
    }
    var pairs;
    if (format === 'social' || format === 'logo') {
      var obj = parse();
      pairs = Object.keys(obj).map(function (k) { return [k, obj[k]]; });
    } else if (format === 'friend') {
      pairs = parse().map(function (o) { return [o.label || '', o.url || '']; });
    } else {
      pairs = parse().map(function (o) {
        var ty = o.type || 'link';
        if (ty === 'categories') ty = 'pages';
        return [o.label || '', o.url || '', ty];
      });
    }
    pairs.forEach(function (p) {
      ed.appendChild(kvBuildRow(ed, format, p[0], p[1], p[2]));
    });
    if (format === 'nav') {
      // 固定内置三项：首页/文章/说说始终存在（可改名、可排序、不可删）
      var seen = {};
      ed.querySelectorAll('.kv-row').forEach(function (row) {
        var t = row.querySelector('.kv-type');
        if (t) seen[t.value] = true;
      });
      KV_TYPES.slice(0, 3).forEach(function (b) {
        if (!seen[b[0]]) ed.appendChild(kvBuildRow(ed, format, b[1], b[2], b[0]));
      });
    } else if (!ed.querySelector('.kv-row')) {
      ed.appendChild(kvBuildRow(ed, format, '', '', ''));
    }
    var add = ed.parentNode.querySelector('.kv-add');
    // 存在空行（名称/平台名为空）时禁用添加按钮，避免堆出多行空行
    function kvUpdateAdd() {
      if (!add) return;
      var empty = false;
      ed.querySelectorAll('.kv-row').forEach(function (row) {
        var k = row.querySelector('.kv-key');
        if (!k || !k.value.trim()) empty = true;
      });
      add.disabled = empty;
    }
    ed.addEventListener('input', kvUpdateAdd);
    ed.addEventListener('click', function (e) {
      if (e.target.closest('.kv-del')) kvUpdateAdd();
    });
    kvUpdateAdd();
    if (add) {
      add.addEventListener('click', function () {
        // 新增导航项只能选「页面」或「链接」（内置三项固定不可新增）
        ed.appendChild(kvBuildRow(ed, format, '', '', format === 'nav' ? 'link' : ''));
        kvUpdateAdd();
      });
    }
    // 所有格式均支持拖拽排序
    kvEnableSort(ed);
    ed._sync = function () {
      var items = [];
      ed.querySelectorAll('.kv-row').forEach(function (row) {
        var k = row.querySelector('.kv-key').value.trim();
        if (!k) return;
        var u = (row.querySelector('.kv-url') || { value: '' }).value.trim();
        if (format === 'nav') {
          var t = row.querySelector('.kv-type').value;
          // 链接/页面保留用户填写/选择的路径；内置类型走预设路径
          u = t === 'link' || t === 'pages' ? u : kvTypeDefaultUrl(t);
          items.push([k, u, t]);
        } else {
          items.push([k, u]);
        }
      });
      if (format === 'social' || format === 'logo') {
        var obj = {};
        items.forEach(function (p) { if (p[0]) obj[p[0]] = p[1]; });
        target.value = JSON.stringify(obj);
      } else if (format === 'friend') {
        target.value = JSON.stringify(items.map(function (p) {
          return { label: p[0], url: p[1] };
        }));
      } else {
        target.value = JSON.stringify(items.map(function (p) {
          return { type: p[2] || 'link', label: p[0], url: p[1] };
        }));
      }
    };
  });

  // ---- 公共图片选择器：上传 / 从附件库选择（站点 Logo 与社交图标共用）----
  (function () {
    'use strict';
    function adminBase() {
      var link = document.querySelector('link[href$="/static/admin.css"]');
      if (!link) return '';
      var href = link.getAttribute('href');
      var i = href.indexOf('/static/admin.css');
      return i > 0 ? href.slice(0, i) : '';
    }
    function openPicker(base, list, callback, fileInput) {
      var overlay = document.createElement('div');
      overlay.className = 'modal-overlay';
      var box = document.createElement('div');
      box.className = 'modal-box picker-box';
      box.setAttribute('role', 'dialog');
      box.setAttribute('aria-modal', 'true');
      var title = document.createElement('h3');
      title.className = 'modal-title';
      title.textContent = '选择图片';
      var head = document.createElement('div');
      head.className = 'picker-head';
      var uploadBtn = document.createElement('button');
      uploadBtn.type = 'button';
      uploadBtn.className = 'btn btn-sm';
      uploadBtn.textContent = '上传图片';
      uploadBtn.addEventListener('click', function () { fileInput.click(); });
      head.appendChild(uploadBtn);
      var grid = document.createElement('div');
      grid.className = 'picker-grid';
      if (!list.length) {
        var empty = document.createElement('p');
        empty.className = 'empty';
        empty.textContent = '附件库暂无图片，请先上传';
        grid.appendChild(empty);
      }
      list.forEach(function (att) {
        var item = document.createElement('button');
        item.type = 'button';
        item.className = 'picker-item';
        item.title = att.name;
        var img = document.createElement('img');
        img.src = att.url;
        img.alt = att.name;
        img.loading = 'lazy';
        item.appendChild(img);
        item.addEventListener('click', function () {
          callback(att.url);
          overlay.remove();
        });
        grid.appendChild(item);
      });
      var actions = document.createElement('div');
      actions.className = 'modal-actions';
      var cancel = document.createElement('button');
      cancel.type = 'button';
      cancel.className = 'btn';
      cancel.textContent = '取消';
      cancel.addEventListener('click', function () { overlay.remove(); });
      actions.appendChild(cancel);
      box.appendChild(title);
      box.appendChild(head);
      box.appendChild(grid);
      box.appendChild(actions);
      overlay.appendChild(box);
      document.body.appendChild(overlay);
      overlay.addEventListener('click', function (e) {
        if (e.target === overlay) overlay.remove();
      });
      return overlay;
    }
    window.hancicPickImage = function (callback) {
      var base = adminBase();
      var fileInput = document.createElement('input');
      fileInput.type = 'file';
      fileInput.accept = 'image/*';
      fileInput.hidden = true;
      document.body.appendChild(fileInput);
      fileInput.addEventListener('change', function () {
        if (!fileInput.files || !fileInput.files.length) return;
        var fd = new FormData();
        fd.append('files', fileInput.files[0]);
        window.hancicFetch('/api/uploads', { method: 'POST', body: fd })
          .then(function (res) { return res.ok ? res.json() : Promise.reject(new Error('上传失败（HTTP ' + res.status + '）')); })
          .then(function (json) {
            if (json.data && json.data[0]) {
              callback(base + '/uploads/' + json.data[0].path);
              document.querySelectorAll('.picker-box').forEach(function (b) { b.closest('.modal-overlay').remove(); });
            }
            fileInput.value = '';
          })
          .catch(function (err) {
            window.hancicToast(err && err.message ? err.message : '上传失败，请重试', 'error');
            fileInput.value = '';
          });
      });
      window.hancicFetch(base + '/admin/api/attachments?kind=image')
        .then(function (res) { return res.ok ? res.json() : Promise.reject(new Error('加载附件失败')); })
        .then(function (list) {
          openPicker(base, list, callback, fileInput);
        })
        .catch(function (err) {
          window.hancicToast(err && err.message ? err.message : '加载附件失败', 'error');
        });
    };
  })();

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

// ---- 文章编辑器（milkdown WYSIWYG）与自动保存 ----
(function () {
  'use strict';

  var editorEl = document.getElementById('editor');
  // milkdown.min.js 在 admin.js 之后加载（layout.html 的 block scripts 在其后），
  // 故等到 DOMContentLoaded（此时所有经典脚本已执行完毕）再初始化编辑器。
  if (!editorEl) return;
  if (window.HancicEditor) {
    initEditor();
  } else {
    document.addEventListener('DOMContentLoaded', initEditor);
  }

  // 新建页本地草稿的 localStorage key（无服务端 autosave，I1）。
  var DRAFT_KEY = 'hancic-draft';

  function initEditor() {
    var form = document.getElementById('post-form');
    var statusInput = document.getElementById('post-status');
    var statusEl = document.getElementById('save-status');
    var editor = null;
    var lastSaved = null;
    // 新建页（_post 无 id）：无服务端 autosave，改走本地草稿（I1）
    var isNewPost = !window._post || !window._post.id;
    // 大文档 setContent 初始化慢：加载期间显示占位，实例创建后移除
    var loadingEl = document.getElementById('editor-loading');
    if (loadingEl) loadingEl.hidden = false;

    // 图片上传：files → POST /api/uploads（hancicFetch 自动带 CSRF 头）。
    // 返回 [{path, orig_name}]，由 milkdown bundle 以 Markdown 图片语法插入光标处；
    // 失败 Toast 提示并返回空数组（bundle 侧不插入）。
    function uploadImages(files) {
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
          return (json.data || []).map(function (att) {
            return { path: att.path, orig_name: att.orig_name };
          });
        })
        .catch(function (err) {
          window.hancicToast(err && err.message ? err.message : '上传失败，请重试', 'error');
          return [];
        });
    }

    function currentContent() {
      return editor ? editor.getMarkdown() : '';
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
    // 由 milkdown 取值补进隐藏字段（编辑器是 div，不会随表单自动提交）。
    // 确认后先 getMarkdownAsync 强制序列化（规避 listener 异步延迟导致丢内容），
    // 写入隐藏字段再 requestSubmit；二次提交（dataset.confirmed）直接补字段。
    if (form) {
      form.addEventListener('submit', function (e) {
        var sb = e.submitter;
        var action = sb && sb.dataset ? sb.dataset.action : null;
        if (action && !form.dataset.confirmed) {
          e.preventDefault();
          var label = action === 'published' ? '发布' : '保存草稿';
          window.hancicConfirm('确认' + label + '吗？', sb).then(function (ok) {
            if (!ok) return;
            form.dataset.confirmed = '1';
            statusInput.value = action;
            var finalize = function (md) {
              var el = form.querySelector('input[name="content_md"]');
              if (!el) {
                el = document.createElement('input');
                el.type = 'hidden';
                el.name = 'content_md';
                form.appendChild(el);
              }
              el.value = md;
              form.requestSubmit(sb);
            };
            if (editor && editor.getMarkdownAsync) {
              editor.getMarkdownAsync().then(finalize);
            } else {
              finalize(currentContent());
            }
          });
          return;
        }
        if (!form.querySelector('input[name="content_md"]')) {
          var md = document.createElement('input');
          md.type = 'hidden';
          md.name = 'content_md';
          md.value = currentContent();
          form.appendChild(md);
        }
      });
    }

    // 初始内容：编辑页取服务端 data-content；新建页优先恢复本地草稿，
    // 无草稿时注入 Markdown 语法模板（window._newPostTemplate，JSON 注入避免属性转义截断）。
    var initial = editorEl.getAttribute('data-content') || '';
    if (isNewPost) {
      try {
        var draft = localStorage.getItem(DRAFT_KEY);
        if (draft) {
          initial = draft;
        } else if (window._newPostTemplate) {
          initial = window._newPostTemplate;
        }
      } catch (e) { /* 忽略 */ }
    }

    // 初始化 milkdown WYSIWYG 编辑器（本地 vendor bundle，无外网依赖）。
    // 文档变化（onUpdate）触发自动保存 / 本地草稿写入。
    window.HancicEditor.create({
      el: editorEl,
      content: initial,
      isNewPost: isNewPost,
      draftKey: DRAFT_KEY,
      onUpdate: autosaveNow,
      onUpload: uploadImages
    }).then(function (inst) {
      editor = inst;
      if (loadingEl) loadingEl.remove();
      // 首次同步 lastSaved，避免初始化即触发“内容变化”
      lastSaved = initial;
      // 已保存文章（有 id）进入编辑页时清掉新建页草稿，防止误恢复（I1）
      if (!isNewPost && window.localStorage) {
        window.localStorage.removeItem(DRAFT_KEY);
      }
      initToolbar();
    });

    // 工具栏：图片上传插入、链接（hancicPrompt）、全屏编辑
    function initToolbar() {
      var imgBtn = document.getElementById('md-insert-img');
      var linkBtn = document.getElementById('md-insert-link');
      var imgInput = document.getElementById('md-file-input');
      var fullscreenBtn = document.getElementById('md-fullscreen');
      var editPanel = document.querySelector('.post-edit-panel');
      if (imgBtn && imgInput) {
        imgBtn.addEventListener('click', function () { imgInput.click(); });
        imgInput.addEventListener('change', function () {
          if (!imgInput.files.length) return;
          uploadImages(Array.prototype.slice.call(imgInput.files)).then(function (atts) {
            atts.forEach(function (a) { if (editor) editor.insertImage(a); });
          });
          imgInput.value = '';
        });
      }
      if (linkBtn) {
        linkBtn.addEventListener('click', function () {
          window.hancicPrompt('链接地址（http/https）', 'https://').then(function (url) {
            if (!url) return;
            if (editor) editor.insertLink(null, url);
            editorEl.focus();
          });
        });
      }
      // 全屏编辑：切换编辑面板全屏（Esc 退出）
      if (fullscreenBtn && editPanel) {
        function setFullscreen(on) {
          editPanel.classList.toggle('hancic-fullscreen', on);
          fullscreenBtn.classList.toggle('active', on);
          fullscreenBtn.setAttribute('title', on ? '退出全屏' : '全屏编辑');
          fullscreenBtn.setAttribute('aria-label', on ? '退出全屏' : '全屏编辑');
          if (on && editor) editorEl.focus();
        }
        fullscreenBtn.addEventListener('click', function () {
          setFullscreen(!editPanel.classList.contains('hancic-fullscreen'));
        });
        document.addEventListener('keydown', function (e) {
          if (e.key === 'Escape' && editPanel.classList.contains('hancic-fullscreen')) {
            setFullscreen(false);
          }
        });
      }
      // 标记命令按钮：H1/H2/H3、加粗、斜体、行内代码、引用、列表、有序列表、代码块、分割线
      function bindCmd(id, fn) {
        var btn = document.getElementById(id);
        if (btn && editor && editor.command) {
          btn.addEventListener('click', function () {
            fn();
            editorEl.focus();
          });
        }
      }
      if (editor && editor.command) {
        var cmd = editor.command;
        bindCmd('md-h1', function () { cmd.heading(1); });
        bindCmd('md-h2', function () { cmd.heading(2); });
        bindCmd('md-h3', function () { cmd.heading(3); });
        bindCmd('md-p', function () { cmd.paragraph(); });
        bindCmd('md-bold', function () { cmd.strong(); });
        bindCmd('md-italic', function () { cmd.emphasis(); });
        bindCmd('md-code', function () { cmd.inlineCode(); });
        bindCmd('md-quote', function () { cmd.blockquote(); });
        bindCmd('md-ul', function () { cmd.bulletList(); });
        bindCmd('md-ol', function () { cmd.orderedList(); });
        bindCmd('md-hr', function () { cmd.hr(); });
        bindCmd('md-table', function () { cmd.table(); });
        bindCmd('md-codeblock', function () {
          window.hancicPrompt('代码块语言（如 python / json / sql，留空为纯文本）', '').then(function (lang) {
            if (lang === null) return;
            cmd.codeBlock(lang);
          });
        });
      }
    }

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


// ---- 文章编辑页：分类选择器（与标签同款输入框 + 建议面板，单选）----
(function () {
  'use strict';
  var input = document.getElementById('post-category');
  var hidden = document.getElementById('post-category-id');
  if (!input || !hidden) return;
  var panel = document.getElementById('cat-suggest-panel');
  var options = [];
  var dataEl = document.getElementById('cat-suggest-data');
  if (dataEl) {
    Array.prototype.forEach.call(dataEl.querySelectorAll('li'), function (li) {
      options.push({ id: li.dataset.id || '', name: li.textContent.trim() });
    });
  }

  function currentId() { return String(hidden.value); }
  function syncDisplay() {
    var cur = null;
    options.forEach(function (o) { if (String(o.id) === currentId()) cur = o; });
    input.value = cur ? cur.name : '';
  }
  function showPanel() {
    if (!panel) return;
    panel.innerHTML = '';
    options.forEach(function (o) {
      var item = document.createElement('button');
      item.type = 'button';
      item.className = 'suggest-item';
      item.textContent = o.name;
      if (String(o.id) === currentId()) item.classList.add('selected');
      item.addEventListener('mousedown', function (e) { e.preventDefault(); });
      item.addEventListener('click', function () {
        hidden.value = o.id;
        input.value = o.name;
        panel.hidden = true;
        input.focus();
      });
      panel.appendChild(item);
    });
    panel.hidden = false;
    panel.scrollTop = 0;
  }
  syncDisplay();
  input.addEventListener('click', showPanel);
  input.addEventListener('focus', showPanel);
  input.addEventListener('blur', function () { setTimeout(function () { if (panel) panel.hidden = true; }, 150); });
  input.addEventListener('keydown', function (e) {
    if (e.key === 'Backspace' || e.key === 'Delete') {
      if (currentId()) {
        hidden.value = '';
        syncDisplay();
        e.preventDefault();
      }
    } else if (e.key === 'Escape') {
      if (panel) panel.hidden = true;
    }
  });
})();

// ---- 文章编辑页：标签 chips + 建议面板（限高滚动，替代 datalist 防高度异常）----
(function () {
  'use strict';
  var editor = document.getElementById('tag-editor');
  var hidden = document.getElementById('tags-hidden');
  var input = document.getElementById('post-tags');
  if (!editor || !hidden || !input) return;
  var chips = document.getElementById('tag-chips');
  var panel = document.getElementById('tag-suggest-panel');
  var allTags = [];
  var dataEl = document.getElementById('tag-suggest-data');
  if (dataEl) {
    Array.prototype.forEach.call(dataEl.querySelectorAll('li'), function (li) {
      var n = li.textContent.trim();
      if (n) allTags.push(n);
    });
  }

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

  // 建议面板：focus 显示全部已有标签，输入过滤；限高 240px 滚动
  function showSuggest(filter) {
    if (!panel) return;
    panel.innerHTML = '';
    var kw = (filter || '').toLowerCase();
    var cur = currentTags();
    var shown = 0;
    allTags.forEach(function (name) {
      if (cur.indexOf(name) !== -1) return; // 已添加的不重复建议
      if (kw && name.toLowerCase().indexOf(kw) === -1) return;
      var item = document.createElement('button');
      item.type = 'button';
      item.className = 'suggest-item';
      item.textContent = name;
      // mousedown 阻止默认，避免 input blur 先触发隐藏面板
      item.addEventListener('mousedown', function (e) { e.preventDefault(); });
      item.addEventListener('click', function () {
        addTag(name);
        input.value = '';
        showSuggest('');
        input.focus();
      });
      panel.appendChild(item);
      shown++;
    });
    panel.hidden = shown === 0;
    panel.scrollTop = 0;
  }
  function hideSuggest() {
    if (panel) panel.hidden = true;
  }

  render();
  input.addEventListener('focus', function () { showSuggest(input.value); });
  input.addEventListener('input', function () { showSuggest(input.value); });
  input.addEventListener('blur', function () {
    // 延迟隐藏，让点击建议项的 mousedown 先执行
    setTimeout(hideSuggest, 150);
  });
  input.addEventListener('keydown', function (e) {
    if (e.key === 'Enter' || e.key === ',') {
      e.preventDefault();
      addTag(input.value);
      input.value = '';
      showSuggest('');
    } else if (e.key === 'Backspace' && !input.value && currentTags().length) {
      var cur = currentTags();
      cur.pop();
      hidden.value = cur.join(',');
      render();
    } else if (e.key === 'Escape') {
      hideSuggest();
    }
  });
  input.addEventListener('change', function () {
    addTag(input.value);
    input.value = '';
    showSuggest('');
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

// ---- 自定义日期选择器（非原生控件）：点击输入框/日历按钮弹出月份日历 ----
(function () {
  'use strict';
  var pickers = document.querySelectorAll('.date-picker');
  if (!pickers.length) return;
  var cal = document.createElement('div');
  cal.className = 'date-cal';
  cal.hidden = true;
  document.body.appendChild(cal);
  var active = null; // { picker, input, year, month, selected }
  var WEEKDAYS = ['日', '一', '二', '三', '四', '五', '六'];

  function pad(n) { return String(n).padStart(2, '0'); }
  function todayStr() {
    var n = new Date();
    return n.getFullYear() + '-' + pad(n.getMonth() + 1) + '-' + pad(n.getDate());
  }
  function render() {
    if (!active) return;
    var y = active.year, m = active.month;
    var first = new Date(y, m, 1);
    var daysInMonth = new Date(y, m + 1, 0).getDate();
    var offset = first.getDay();
    var today = todayStr();
    var html = '<div class="date-cal-head">' +
      '<button type="button" class="date-cal-nav" data-nav="-1" aria-label="上月">‹</button>' +
      '<span class="date-cal-title">' + y + ' 年 ' + (m + 1) + ' 月</span>' +
      '<button type="button" class="date-cal-nav" data-nav="1" aria-label="下月">›</button>' +
      '</div><div class="date-cal-grid">';
    for (var w = 0; w < 7; w++) html += '<span class="date-cal-wd">' + WEEKDAYS[w] + '</span>';
    for (var i = 0; i < offset; i++) html += '<span class="date-cal-empty"></span>';
    for (var d = 1; d <= daysInMonth; d++) {
      var val = y + '-' + pad(m + 1) + '-' + pad(d);
      html += '<button type="button" class="date-cal-day' +
        (val === today ? ' today' : '') +
        (val === active.selected ? ' selected' : '') +
        '" data-date="' + val + '">' + d + '</button>';
    }
    html += '</div>';
    cal.innerHTML = html;
  }
  function openPicker(picker) {
    var input = picker.querySelector('.date-input');
    var mm = (input.value || '').trim().match(/^(\d{4})-(\d{2})-(\d{2})$/);
    var now = new Date();
    active = {
      picker: picker,
      input: input,
      year: mm ? +mm[1] : now.getFullYear(),
      month: mm ? +mm[2] - 1 : now.getMonth(),
      selected: mm ? input.value.trim() : ''
    };
    render();
    var rect = picker.getBoundingClientRect();
    cal.style.left = Math.min(Math.max(8, rect.left), window.innerWidth - 292) + 'px';
    cal.style.top = (rect.bottom + 6) + 'px';
    cal.hidden = false;
  }
  function closeCal() {
    cal.hidden = true;
    active = null;
  }
  // 弹层事件（单例委托）
  cal.addEventListener('click', function (e) {
    var nav = e.target.closest('.date-cal-nav');
    if (nav) {
      active.month += parseInt(nav.dataset.nav, 10);
      if (active.month < 0) { active.month = 11; active.year--; }
      if (active.month > 11) { active.month = 0; active.year++; }
      render();
      return;
    }
    var day = e.target.closest('.date-cal-day');
    if (day && active) {
      active.input.value = day.dataset.date;
      closeCal();
    }
  });
  pickers.forEach(function (picker) {
    var input = picker.querySelector('.date-input');
    var btn = picker.querySelector('.date-cal-btn');
    function open() { openPicker(picker); }
    if (btn) btn.addEventListener('click', open);
    if (input) input.addEventListener('click', open);
  });
  document.addEventListener('click', function (e) {
    if (!e.target.closest('.date-picker') && !cal.hidden) closeCal();
  });
  document.addEventListener('keydown', function (e) {
    if (e.key === 'Escape' && !cal.hidden) closeCal();
  });
})();

// ---- 文件选择（备份包/主题 zip）：显示已选文件名 ----
(function () {
  'use strict';
  document.querySelectorAll('input[type="file"].file-input-sr').forEach(function (input) {
    var nameEl = document.getElementById(input.id + '-name');
    if (!nameEl) return;
    input.addEventListener('change', function () {
      nameEl.textContent = input.files && input.files[0] ? input.files[0].name : '未选择';
    });
  });
})();
