// 后台全局脚本：CSRF 注入、data-confirm 确认弹窗、抽屉导航（≤768px）、
// 侧栏折叠（桌面端）、导航图标、仪表盘趋势图与设置页键值编辑器、明暗模式切换。
(function () {
  'use strict';

  // ---- 明暗模式切换：localStorage 持久化，Chart.js 联动 ----
  var MODE_KEY = 'admin-mode';

  // ---- 密码可见切换（规范 7.4.3）：事件委托，任意 .auth-eye 生效（登录/初始化/后台改密） ----
  document.addEventListener('click', function (e) {
    var eye = e.target.closest('.auth-eye');
    if (!eye) return;
    var wrap = eye.closest('.auth-input-wrap');
    var input = wrap ? wrap.querySelector('input[type="password"], input[type="text"]') : null;
    if (!input) return;
    var show = input.type === 'password';
    input.type = show ? 'text' : 'password';
    eye.classList.toggle('visible', show);
    eye.setAttribute('aria-label', show ? '隐藏密码' : '显示密码');
    input.focus();
  });
  function currentMode() {
    return document.documentElement.getAttribute('data-mode') || 'dark';
  }
  // 暴露给编辑器等独立 IIFE 使用（明暗切换联动）
  window.hancicMode = currentMode;
  function setMode(mode) {
    document.documentElement.setAttribute('data-mode', mode);
    try { localStorage.setItem(MODE_KEY, mode); } catch (e) { /* 忽略 */ }
    // 联动 Chart.js + 地图：配色随明暗令牌（accent/ink 两套取值）重绘
    paintChartTheme(window._adminChart);
    paintMapTheme(window._adminMap);
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

  // ---- 主题配色切换：内置 5 主题（规范 03：indigo/emerald/rose/amber/slate，默认 emerald）
  // localStorage 持久化；旧版 5 色名（pink/blue/green/purple/orange）自动迁移映射 ----
  var ACCENT_KEY = 'admin-accent';
  var ACCENTS = ['emerald', 'indigo', 'rose', 'amber', 'slate'];
  var ACCENT_LEGACY = { pink: 'rose', blue: 'indigo', green: 'emerald', purple: 'indigo', orange: 'amber' };
  function currentAccent() {
    return document.documentElement.getAttribute('data-accent') || 'emerald';
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
    // 联动 Chart.js + 地图：主序列/填充随主题色重绘（仅仪表盘有图）
    paintChartTheme(window._adminChart);
    paintMapTheme(window._adminMap);
  }
  var savedAccent;
  try { savedAccent = localStorage.getItem(ACCENT_KEY); } catch (e) { savedAccent = null; }
  if (savedAccent) {
    if (ACCENT_LEGACY.hasOwnProperty(savedAccent)) savedAccent = ACCENT_LEGACY[savedAccent]; // 旧名迁移
    if (ACCENTS.indexOf(savedAccent) === -1) savedAccent = null; // 未知值回落默认
  }
  if (savedAccent) {
    document.documentElement.setAttribute('data-accent', savedAccent);
  } else {
    document.documentElement.setAttribute('data-accent', 'emerald');
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
  // ---- 确认对话框 / 输入对话框 / Toast（规范 7.10 ConfirmDialog / 7.11 Toast）----
  (function () {
    'use strict';
    var overlay = null;
    var state = null; // { resolve, done, triggerEl, kind }

    var DESTRUCTIVE = ['删除', '清空', '吊销', '恢复', '卸载', '移除', '覆盖', '重置', '退出', '清理', '解散'];
    var ACTION_WORDS = ['删除', '清空', '吊销', '恢复', '卸载', '移除', '覆盖', '重置', '退出', '清理', '发布', '下架', '切换', '启用', '禁用', '保存', '更新', '重置', '解散'];
    function actionOf(msg) {
      for (var i = 0; i < ACTION_WORDS.length; i++) {
        if (msg.indexOf(ACTION_WORDS[i]) > -1) return ACTION_WORDS[i];
      }
      return '';
    }
    function isDestructive(msg) {
      for (var i = 0; i < DESTRUCTIVE.length; i++) {
        if (msg.indexOf(DESTRUCTIVE[i]) > -1) return true;
      }
      return false;
    }
    var SVG_ALERT = '<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>';
    var SVG_X = '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>';

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
      var head = document.createElement('div');
      head.className = 'modal-head';
      var iconEl = document.createElement('span');
      iconEl.className = 'modal-icon';
      iconEl.hidden = true;
      var titleEl = document.createElement('h3');
      titleEl.className = 'modal-title';
      titleEl.id = 'modal-title';
      var closeBtn = document.createElement('button');
      closeBtn.type = 'button';
      closeBtn.className = 'modal-close';
      closeBtn.setAttribute('aria-label', '关闭');
      closeBtn.title = '关闭';
      closeBtn.innerHTML = SVG_X;
      head.appendChild(iconEl);
      head.appendChild(titleEl);
      head.appendChild(closeBtn);
      var body = document.createElement('div');
      body.className = 'modal-body';
      var msgEl = document.createElement('p');
      msgEl.className = 'modal-msg';
      body.appendChild(msgEl);
      var actionsEl = document.createElement('div');
      actionsEl.className = 'modal-actions';
      box.appendChild(head);
      box.appendChild(body);
      box.appendChild(actionsEl);
      overlay.appendChild(box);
      document.body.appendChild(overlay);
      overlay.addEventListener('click', function (e) {
        if (e.target === overlay) settle(null);
      });
      closeBtn.addEventListener('click', function () { settle(null); });
      document.addEventListener('keydown', function (e) {
        if (!overlay.hidden && e.key === 'Escape') settle(null);
      });
    }

    // 统一收尾：resolve(val)；取消类统一传 null
    function settle(val) {
      if (!state || state.done) return;
      state.done = true;
      overlay.hidden = true;
      if (state.triggerEl instanceof Element && state.triggerEl.isConnected) {
        state.triggerEl.focus();
      }
      var resolve = state.resolve;
      var kind = state.kind;
      state = null;
      resolve(kind === 'confirm' ? val !== null : val);
    }

    function resetBox() {
      var box = overlay.querySelector('.modal-box');
      var iconEl = box.querySelector('.modal-icon');
      var titleEl = box.querySelector('.modal-title');
      var msgEl = box.querySelector('.modal-msg');
      var actionsEl = box.querySelector('.modal-actions');
      iconEl.hidden = true;
      iconEl.innerHTML = '';
      msgEl.innerHTML = '';
      actionsEl.innerHTML = '';
      return { iconEl: iconEl, titleEl: titleEl, msgEl: msgEl, actionsEl: actionsEl };
    }

    // 确认对话框：hancicConfirm(message, triggerEl) → Promise<boolean>
    window.hancicConfirm = function (message, triggerEl) {
      ensure();
      if (state) state.resolve(null);
      return new Promise(function (resolve) {
        state = { resolve: resolve, done: false, triggerEl: triggerEl, kind: 'confirm' };
        var box = overlay.querySelector('.modal-box');
        var els = resetBox();
        var verb = actionOf(message);
        var danger = isDestructive(message);
        els.titleEl.textContent = verb ? verb + '确认' : '操作确认';
        if (danger) {
          els.iconEl.hidden = false;
          els.iconEl.innerHTML = SVG_ALERT;
        }
        els.msgEl.textContent = message;
        var cancel = document.createElement('button');
        cancel.type = 'button';
        cancel.className = 'btn';
        cancel.textContent = '取消';
        var ok = document.createElement('button');
        ok.type = 'button';
        ok.className = danger ? 'btn btn-danger' : 'btn btn-primary';
        ok.textContent = verb || '确认';
        els.actionsEl.appendChild(cancel);
        els.actionsEl.appendChild(ok);
        ok.addEventListener('click', function () { settle(true); });
        cancel.addEventListener('click', function () { settle(null); });
        overlay.hidden = false;
        ok.focus();
      });
    };

    // 输入对话框：hancicPrompt(message, defaultValue) → Promise<string|null>
    window.hancicPrompt = function (message, defaultValue) {
      ensure();
      if (state) state.resolve(null);
      return new Promise(function (resolve) {
        state = { resolve: resolve, done: false, triggerEl: null, kind: 'prompt' };
        var els = resetBox();
        els.titleEl.textContent = message;
        var input = document.createElement('input');
        input.type = 'text';
        input.className = 'modal-input';
        input.value = defaultValue || '';
        els.msgEl.appendChild(input);
        var cancel = document.createElement('button');
        cancel.type = 'button';
        cancel.className = 'btn';
        cancel.textContent = '取消';
        var ok = document.createElement('button');
        ok.type = 'button';
        ok.className = 'btn btn-primary';
        ok.textContent = '确定';
        els.actionsEl.appendChild(cancel);
        els.actionsEl.appendChild(ok);
        function finish() { settle(input.value.trim()); }
        ok.addEventListener('click', finish);
        cancel.addEventListener('click', function () { settle(null); });
        input.addEventListener('keydown', function (e) {
          if (e.key === 'Enter') { e.preventDefault(); finish(); }
        });
        overlay.hidden = false;
        input.focus();
        input.select();
      });
    };

    // ---- Toast（规范 7.11：4 级，顶部居中堆叠）----
    var toastBox = null;
    var TOAST_ICONS = {
      success: '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"/><polyline points="22 4 12 14.01 9 11.01"/></svg>',
      error: '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="10"/><line x1="15" y1="9" x2="9" y2="15"/><line x1="9" y1="9" x2="15" y2="15"/></svg>',
      warn: '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>',
      info: '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/></svg>'
    };
    function ensureToast() {
      if (toastBox) return;
      toastBox = document.createElement('div');
      toastBox.className = 'toast-container';
      document.body.appendChild(toastBox);
    }
    function removeToast(t) {
      if (!t.parentNode) return;
      t.classList.add('toast-out');
      setTimeout(function () { if (t.parentNode) t.parentNode.removeChild(t); }, 220);
    }
    window.hancicToast = function (message, type) {
      ensureToast();
      var level = type === 'error' || type === 'warn' || type === 'info' || type === 'success' ? type : 'success';
      var t = document.createElement('div');
      t.className = 'toast toast-' + level;
      t.setAttribute('role', level === 'error' || level === 'warn' ? 'alert' : 'status');
      t.insertAdjacentHTML('afterbegin', TOAST_ICONS[level]);
      var span = document.createElement('span');
      span.textContent = message;
      t.appendChild(span);
      // error/warn 手动关闭（规范：不自动消失）；success/info 3 秒自动消失
      if (level === 'error' || level === 'warn') {
        var close = document.createElement('button');
        close.type = 'button';
        close.className = 'toast-close';
        close.setAttribute('aria-label', '关闭');
        close.innerHTML = SVG_X;
        close.addEventListener('click', function () { removeToast(t); });
        t.appendChild(close);
      } else {
        setTimeout(function () { removeToast(t); }, 3000);
      }
      toastBox.appendChild(t);
      // 最多同时 3 条，超出移除最早
      while (toastBox.children.length > 3) {
        toastBox.removeChild(toastBox.firstChild);
      }
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
    '/admin/columns': '<path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20"/><path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z"/>',
    '/admin/trails': '<circle cx="6" cy="19" r="3"/><path d="M9 19h8.5a3.5 3.5 0 0 0 0-7h-11a3.5 3.5 0 0 1 0-7H15"/>',
    '/admin/settings': '<line x1="4" y1="21" x2="4" y2="14"/><line x1="4" y1="10" x2="4" y2="3"/><line x1="12" y1="21" x2="12" y2="12"/><line x1="12" y1="8" x2="12" y2="3"/><line x1="20" y1="21" x2="20" y2="16"/><line x1="20" y1="12" x2="20" y2="3"/><line x1="1" y1="14" x2="7" y2="14"/><line x1="9" y1="8" x2="15" y2="8"/><line x1="17" y1="16" x2="23" y2="16"/>',
    '/admin/themes': '<path d="M12 22a10 10 0 1 1 10-10c0 2.2-1.8 4-4 4h-2a3 3 0 0 0-3 3c0 1.5 1 3 3 3z"/><circle cx="7.5" cy="11.5" r="1"/><circle cx="11" cy="7.5" r="1"/><circle cx="15.5" cy="9.5" r="1"/><circle cx="17.5" cy="13.5" r="1"/>',
    '/admin/stats': '<line x1="12" y1="20" x2="12" y2="10"/><line x1="18" y1="20" x2="18" y2="4"/><line x1="6" y1="20" x2="6" y2="16"/>',
    '/admin/tokens': '<path d="M21 2l-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.778 7.778 5.5 5.5 0 0 1 7.777-7.777zm0 0L15.5 7.5m0 0l3 3L22 7l-3-3m-3.5 3.5L19 4"/>',
    '/admin/backup': '<polyline points="21 8 21 21 3 21 3 8"/><rect x="1" y="3" width="22" height="5"/><line x1="10" y1="12" x2="14" y2="12"/>',
    '/admin/help': '<circle cx="12" cy="12" r="10"/><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3"/><line x1="12" y1="17" x2="12.01" y2="17"/>',
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
    ['column', '专栏', '/columns'],
    ['trail', '轨迹', '/trails'],
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
  function kvBuildRow(ed, format, keyVal, urlVal, typeVal, hiddenVal) {
    var row = document.createElement('div');
    row.className = 'kv-row';
    if (hiddenVal === '1') row.classList.add('kv-hidden');
    // 内置导航项（首页/文章/说说）：固定存在、类型锁定、不可删除，仅可改名与排序
    var builtin = format === 'nav' && ['home', 'articles', 'moments', 'column', 'trail'].indexOf(typeVal) !== -1;
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
        var newRow = kvBuildRow(ed, format, key.value.trim(), url.value.trim(), typeSel.value, row.dataset.hidden === '1' ? '1' : '0');
        row.replaceWith(newRow);
      });
    }
    if (format === 'nav' && typeVal !== 'home' && typeVal !== 'articles') {
      // 行级「隐藏/显示」：前台导航不渲染隐藏项（配置保留，可随时恢复）。
      // 首页与文章为站点基础入口，不允许隐藏，不展示此开关。
      var vis = document.createElement('button');
      vis.type = 'button';
      vis.className = 'kv-vis';
      var SVG_EYE = '<svg xmlns="http://www.w3.org/2000/svg" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/></svg>';
      var SVG_EYE_OFF = '<svg xmlns="http://www.w3.org/2000/svg" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24"/><line x1="1" y1="1" x2="23" y2="23"/></svg>';
      function paintVis() {
        var hidden = row.dataset.hidden === '1';
        row.classList.toggle('kv-hidden', hidden);
        vis.setAttribute('aria-pressed', String(!hidden));
        vis.setAttribute('aria-label', hidden ? '显示此项' : '隐藏此项（前台不显示）');
        vis.title = hidden ? '显示此项' : '隐藏此项';
        vis.innerHTML = hidden ? SVG_EYE_OFF : SVG_EYE;
      }
      row.dataset.hidden = hiddenVal === '1' ? '1' : '0';
      vis.addEventListener('click', function () {
        row.dataset.hidden = row.dataset.hidden === '1' ? '0' : '1';
        paintVis();
      });
      paintVis();
      row.appendChild(vis);
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
        var h = o.hidden === true || o.hidden === '1' || o.hidden === 1 ? '1' : '0';
        return [o.label || '', o.url || '', ty, h];
      });
    }
    pairs.forEach(function (p) {
      ed.appendChild(kvBuildRow(ed, format, p[0], p[1], p[2], p[3]));
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
          var hidden = row.dataset.hidden === '1';
          items.push([k, u, t, hidden]);
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
          return { type: p[2] || 'link', label: p[0], url: p[1], hidden: !!p[3] };
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

  // ---- 仪表盘趋势图（Chart.js）：阅读量 + 点赞数双线 ----
  // 配色读 CSS 令牌（规范 7.9 + 03）：主序列 --accent（随明暗在 500/600 间取）、
  // 填充 --accent-glow、次序列 --ink-500（中性，虚线区分）；切明暗/切 accent 由
  // paintChartTheme 重绘（本文件顶部 setMode/applyAccent 调用）。
  function readCssVar(name, fallback) {
    var v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
    return v || fallback;
  }
  function chartPalette() {
    var isLight = currentMode() === 'light';
    return {
      accent: readCssVar('--accent', isLight ? '#059669' : '#10b981'),
      accentFill: readCssVar('--accent-glow', 'rgba(16, 185, 129, 0.12)'),
      second: readCssVar('--ink-500', isLight ? '#64748B' : '#7E8BA6'),
      grid: isLight ? 'rgba(0,0,0,0.06)' : 'rgba(255,255,255,0.05)',
      tick: readCssVar('--ink-400', isLight ? '#94A3B8' : '#5B6B8C')
    };
  }
  function paintChartTheme(chart) {
    if (!chart) return;
    var p = chartPalette();
    chart.data.datasets[0].borderColor = p.accent;
    chart.data.datasets[0].backgroundColor = p.accentFill;
    chart.data.datasets[0].pointBackgroundColor = p.accent;
    chart.data.datasets[1].borderColor = p.second;
    chart.data.datasets[1].pointBackgroundColor = p.second;
    chart.options.scales.x.grid.color = p.grid;
    chart.options.scales.x.ticks.color = p.tick;
    chart.options.scales.y.grid.color = p.grid;
    chart.options.scales.y.ticks.color = p.tick;
    chart.update('none');
  }
  // ---- 跳转来源饼图（Chart.js Doughnut）与地区分布地图（ECharts 全球 choropleth）----
  // 与趋势图同属仪表盘：数据由 dashboard.html 注入 window.chartSources / window.regionData。
  // 饼图扇区用固定柔和调色板（多分类不适合随单 accent）；地图按国家聚合着色，
  // 色阶随 --accent 令牌，切明暗/切 accent 由 paintMapTheme 重绘（顶部 setMode/applyAccent 调用）。
  var SOURCE_COLORS = ['#10B981', '#6366F1', '#F59E0B', '#EC4899', '#06B6D4',
    '#8B5CF6', '#F43F5E', '#84CC16', '#F97316', '#64748B'];
  // ip2region 国家名（中文为主，个别英文）→ ECharts 世界地图（Natural Earth 英文名）。
  // 流量主要覆盖国内 + 常见海外访问源；未收录国家落下方明细表，不进图。
  var COUNTRY_ALIAS = {
    '中国': 'China', '美国': 'United States', '日本': 'Japan', '韩国': 'Korea',
    '朝鲜': 'Dem. Rep. Korea', '英国': 'United Kingdom', '德国': 'Germany', '法国': 'France',
    '俄罗斯': 'Russia', '加拿大': 'Canada', '澳大利亚': 'Australia', '印度': 'India',
    '新加坡': 'Singapore', '意大利': 'Italy', '西班牙': 'Spain', '荷兰': 'Netherlands',
    '瑞典': 'Sweden', '瑞士': 'Switzerland', '巴西': 'Brazil', '墨西哥': 'Mexico',
    '泰国': 'Thailand', '越南': 'Vietnam', '马来西亚': 'Malaysia', '菲律宾': 'Philippines',
    '印度尼西亚': 'Indonesia', '缅甸': 'Myanmar', '柬埔寨': 'Cambodia', '老挝': 'Laos',
    '蒙古': 'Mongolia', '哈萨克斯坦': 'Kazakhstan', '新西兰': 'New Zealand',
    '爱尔兰': 'Ireland', '葡萄牙': 'Portugal', '比利时': 'Belgium', '奥地利': 'Austria',
    '挪威': 'Norway', '丹麦': 'Denmark', '芬兰': 'Finland', '波兰': 'Poland',
    '乌克兰': 'Ukraine', '捷克': 'Czech Rep.', '希腊': 'Greece', '土耳其': 'Turkey',
    '以色列': 'Israel', '沙特阿拉伯': 'Saudi Arabia', '阿联酋': 'United Arab Emirates',
    '阿根廷': 'Argentina', '智利': 'Chile', '秘鲁': 'Peru', '哥伦比亚': 'Colombia',
    '埃及': 'Egypt', '南非': 'South Africa', '肯尼亚': 'Kenya', '尼日利亚': 'Nigeria',
    '巴基斯坦': 'Pakistan', '孟加拉国': 'Bangladesh', '斯里兰卡': 'Sri Lanka',
    '伊朗': 'Iran', '伊拉克': 'Iraq', '阿富汗': 'Afghanistan', '中国台湾': 'China',
    '中国香港': 'China', '中国澳门': 'China', '香港': 'China', '澳门': 'China', '台湾': 'China'
  };
  function worldFeatureNames() {
    var set = {};
    (window.HANCIC_WORLD_GEO && window.HANCIC_WORLD_GEO.features || []).forEach(function (f) {
      set[f.properties.name] = true;
    });
    return set;
  }
  // 从 regionData（country/province/count）按国家聚合；中英名归一化后仅保留地图可匹配的行
  function countryMapData(featureNames) {
    var map = {};
    var zh = {};
    (window.regionData || []).forEach(function (r) {
      var raw = String(r.country || '').trim();
      if (!raw || raw === '本地' || raw === '未知') return;
      var name = COUNTRY_ALIAS[raw] || raw;
      if (!featureNames[name]) return;
      map[name] = (map[name] || 0) + (r.count || 0);
      if (!zh[name]) zh[name] = raw;
    });
    var data = Object.keys(map)
      .sort(function (a, b) { return map[b] - map[a]; })
      .map(function (name) { return { name: name, value: map[name], zh: zh[name] }; });
    return { data: data, max: data.reduce(function (m, d) { return d.value > m ? d.value : m; }, 1) };
  }
  function mapTheme() {
    var isLight = currentMode() === 'light';
    return {
      accent: readCssVar('--accent', isLight ? '#059669' : '#10b981'),
      low: readCssVar('--accent-soft', isLight ? 'rgba(5, 150, 105, 0.18)' : 'rgba(16, 185, 129, 0.2)'),
      land: readCssVar('--surface-2', isLight ? '#F3F5F9' : '#0B1220'),
      border: readCssVar('--surface-4', isLight ? '#CBD5E1' : '#2C3E5F'),
      tooltipBg: readCssVar('--surface-0', isLight ? '#FFFFFF' : '#111A2C'),
      tooltipBorder: readCssVar('--surface-3', isLight ? '#E4E9F1' : '#1E2A40'),
      ink: readCssVar('--ink-900', isLight ? '#1E293B' : '#E5E9F2'),
      subInk: readCssVar('--ink-500', isLight ? '#64748B' : '#7E8BA6')
    };
  }
  function buildMapOption(ctx, t) {
    var hasData = ctx.data.length > 0;
    return {
      backgroundColor: 'transparent',
      tooltip: {
        trigger: 'item',
        // 挂到 body 层：容器 .map-wrap 为圆角带 overflow:hidden，不挂载时提示框会被裁切
        appendToBody: true,
        transitionDuration: 0.1,
        backgroundColor: t.tooltipBg,
        borderColor: t.tooltipBorder,
        textStyle: { color: t.ink, fontSize: 13 },
        extraCssText: 'z-index: 9999; max-width: 320px; white-space: normal;',
        formatter: function (p) {
          var zh = p.data && p.data.zh;
          var v = p.value;
          // 无数据的国家 ECharts 给 NaN，只显示国名
          if (v === undefined || v === null || (typeof v === 'number' && isNaN(v))) return zh || p.name;
          return (zh || p.name) + '：' + v + ' 次阅读';
        }
      },
      visualMap: {
        show: hasData,
        min: 0,
        max: ctx.max,
        left: 14,
        bottom: 10,
        text: ['高', '低'],
        itemHeight: 90,
        calculable: false,
        textStyle: { color: t.subInk },
        inRange: { color: [t.low, t.accent] }
      },
      series: [{
        type: 'map',
        map: 'world',
        roam: false,
        label: { show: false },
        itemStyle: {
          areaColor: t.land,
          borderColor: t.border,
          borderWidth: 0.6
        },
        emphasis: {
          label: { show: false },
          itemStyle: { areaColor: t.accent }
        },
        select: { disabled: true },
        data: ctx.data
      }]
    };
  }
  function paintMapTheme(map) {
    if (!map || !window._worldMapCtx) return;
    map.setOption(buildMapOption(window._worldMapCtx, mapTheme()), true);
  }
  document.addEventListener('DOMContentLoaded', function () {
    var canvas = document.getElementById('trend');
    if (canvas && window.Chart && window.chartData) {
      window._adminChart = new window.Chart(canvas, {
        type: 'line',
        data: {
          labels: window.chartData.labels,
          datasets: [{
            label: '阅读量',
            data: window.chartData.views,
            fill: true,
            tension: 0.3,
            pointRadius: 2,
            borderWidth: 2
          }, {
            label: '点赞数',
            data: window.chartData.likes,
            fill: false,
            tension: 0.3,
            pointRadius: 2,
            borderWidth: 2,
            borderDash: [5, 4]
          }]
        },
        options: {
          responsive: true,
          maintainAspectRatio: false,
          plugins: {
            legend: {
              display: true,
              labels: {
                boxWidth: 14,
                usePointStyle: true,
                padding: 12
              }
            }
          },
          scales: {
            x: { grid: {}, ticks: {} },
            y: { beginAtZero: true, ticks: { precision: 0 }, grid: {} }
          }
        }
      });
      paintChartTheme(window._adminChart);
    }
    // 跳转来源饼图
    var sourceCanvas = document.getElementById('source-chart');
    if (sourceCanvas && window.Chart && window.chartSources && window.chartSources.length) {
      var labels = [];
      var counts = [];
      window.chartSources.forEach(function (s) {
        labels.push(s.label);
        counts.push(s.count);
      });
      window._adminSourceChart = new window.Chart(sourceCanvas, {
        type: 'doughnut',
        data: {
          labels: labels,
          datasets: [{ data: counts, backgroundColor: SOURCE_COLORS }]
        },
        options: {
          responsive: true,
          maintainAspectRatio: false,
          cutout: '55%',
          plugins: {
            legend: {
              position: 'right',
              labels: { boxWidth: 12, usePointStyle: true, padding: 10 }
            },
            tooltip: {
              callbacks: {
                label: function (ctx) {
                  var total = ctx.dataset.data.reduce(function (a, b) { return a + b; }, 0);
                  var pct = total ? Math.round(ctx.parsed / total * 100) : 0;
                  return ' ' + ctx.label + '：' + ctx.parsed + ' 次（' + pct + '%）';
                }
              }
            }
          }
        }
      });
    }
    // 地区分布地图（全球 choropleth；无匹配国家数据时隐藏容器，明细表仍在）
    var mapEl = document.getElementById('region-map');
    if (mapEl && window.echarts && window.HANCIC_WORLD_GEO) {
      var names = worldFeatureNames();
      var ctx = countryMapData(names);
      if (!ctx.data.length) {
        mapEl.hidden = true;
        return;
      }
      window.echarts.registerMap('world', window.HANCIC_WORLD_GEO);
      var map = window.echarts.init(mapEl);
      window._worldMapCtx = ctx;
      window._adminMap = map;
      map.setOption(buildMapOption(ctx, mapTheme()));
      window.addEventListener('resize', function () { map.resize(); });
    }
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
      // 全屏编辑：编辑区全屏，顶部工具栏默认隐藏（悬停顶部显示），可固定常驻（Esc 退出）
      if (fullscreenBtn && editPanel) {
        var toolbarEl = editPanel.querySelector('.md-toolbar');
        var pinBtn = document.getElementById('md-pin-toolbar');
        var pinned = false;

        function setPinned(p) {
          pinned = p;
          editPanel.classList.toggle('md-toolbar-pinned', p);
          if (pinBtn) {
            pinBtn.classList.toggle('active', p);
            pinBtn.setAttribute('title', p ? '取消固定工具栏' : '固定工具栏');
            pinBtn.setAttribute('aria-label', p ? '取消固定工具栏' : '固定工具栏');
          }
        }
        function showToolbar() { if (!pinned) editPanel.classList.add('md-toolbar-visible'); }
        function hideToolbar() { if (!pinned) editPanel.classList.remove('md-toolbar-visible'); }
        function onMove(e) {
          var rect = editPanel.getBoundingClientRect();
          if (e.clientY - rect.top < 72) showToolbar(); else hideToolbar();
        }
        function onEnter() { showToolbar(); }
        function onLeave() { hideToolbar(); }
        function onEscape(e) {
          if (e.key === 'Escape' && editPanel.classList.contains('hancic-fullscreen')) {
            setFullscreen(false);
          }
        }

        function setFullscreen(on) {
          editPanel.classList.toggle('hancic-fullscreen', on);
          fullscreenBtn.classList.toggle('active', on);
          fullscreenBtn.setAttribute('title', on ? '退出全屏' : '全屏编辑');
          fullscreenBtn.setAttribute('aria-label', on ? '退出全屏' : '全屏编辑');
          if (on) {
            if (pinBtn) pinBtn.hidden = false;
            setPinned(false);
            editPanel.addEventListener('mousemove', onMove);
            editPanel.addEventListener('mouseenter', onEnter);
            editPanel.addEventListener('mouseleave', onLeave);
            document.addEventListener('keydown', onEscape);
          } else {
            if (pinBtn) pinBtn.hidden = true;
            setPinned(false);
            editPanel.removeEventListener('mousemove', onMove);
            editPanel.removeEventListener('mouseenter', onEnter);
            editPanel.removeEventListener('mouseleave', onLeave);
            document.removeEventListener('keydown', onEscape);
            hideToolbar();
          }
          if (on && editor) editorEl.focus();
        }
        fullscreenBtn.addEventListener('click', function () {
          setFullscreen(!editPanel.classList.contains('hancic-fullscreen'));
        });
        if (pinBtn) {
          pinBtn.addEventListener('click', function () { setPinned(!pinned); });
        }
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
    // data-search 启用关键词搜索：菜单顶部插入搜索框，输入时过滤选项
    if (wrap.hasAttribute('data-search')) {
      var search = document.createElement('input');
      search.type = 'text';
      search.className = 'cs-search';
      search.placeholder = '搜索…';
      search.setAttribute('aria-label', '搜索选项');
      search.addEventListener('input', function () {
        var q = search.value.trim().toLowerCase();
        Array.prototype.forEach.call(menu.querySelectorAll('.cs-option'), function (li) {
          li.classList.toggle('hidden', !!q && li.textContent.toLowerCase().indexOf(q) === -1);
        });
      });
      menu.insertBefore(search, menu.firstChild);
    }
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
      document.querySelectorAll('.cs-menu').forEach(function (m) { m.hidden = true; m.classList.remove('cs-up'); });
      if (open) {
        menu.hidden = true;
        btn.setAttribute('aria-expanded', 'false');
      } else {
        menu.hidden = false;
        menu.classList.remove('cs-up');
        // 底部空间不足且上方足够时向上弹出，避免页面底部卡片的下拉被裁剪
        var r = btn.getBoundingClientRect();
        var mh = menu.offsetHeight;
        if (window.innerHeight - r.bottom < mh && r.top > mh) {
          menu.classList.add('cs-up');
        }
        btn.setAttribute('aria-expanded', 'true');
      }
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
    // 点击外部关闭（菜单内部点击不关闭，避免搜索框聚焦即收起）
    document.addEventListener('click', function (e) {
      if (!menu.hidden && !menu.contains(e.target)) {
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

// ---- 右下角悬浮按钮组（#fab-group，登录/初始化页）：
// 默认折叠为 brand 主按钮；hover/focus 展开子按钮组，移出悬浮区约 180ms 自动收起；
// 点外部 / Esc 立即收起；触摸设备点击主按钮切换展开。
(function () {
  'use strict';
  var group = document.getElementById('fab-group');
  if (!group) return;
  var mainBtn = document.getElementById('fab-main');
  var panel = document.getElementById('accent-panel');
  var open = false;
  var closeTimer = null;

  function setOpen(v) {
    if (open === v) return;
    open = v;
    group.classList.toggle('fab-open', v);
    if (mainBtn) mainBtn.setAttribute('aria-expanded', v ? 'true' : 'false');
  }
  function scheduleClose() {
    cancelClose();
    closeTimer = setTimeout(function () { setOpen(false); }, 180);
  }
  function cancelClose() {
    if (closeTimer) { clearTimeout(closeTimer); closeTimer = null; }
  }
  // 悬浮区 = 按钮组 + 主题色气泡（气泡相对按钮向左展开，属于悬浮区一部分）
  function inZone(el) {
    return !!el && (group.contains(el) || (panel && panel.contains(el)));
  }
  group.addEventListener('mouseenter', function () { cancelClose(); setOpen(true); });
  group.addEventListener('mouseleave', function (e) {
    if (!inZone(e.relatedTarget)) scheduleClose();
  });
  if (panel) {
    panel.addEventListener('mouseenter', function () { cancelClose(); });
    panel.addEventListener('mouseleave', function (e) {
      if (!inZone(e.relatedTarget)) scheduleClose();
    });
  }
  group.addEventListener('focusin', function () { cancelClose(); setOpen(true); });
  group.addEventListener('focusout', function (e) {
    if (!inZone(e.relatedTarget)) scheduleClose();
  });
  if (mainBtn) {
    mainBtn.addEventListener('click', function (e) {
      e.stopPropagation();
      setOpen(!open);
    });
  }
  document.addEventListener('click', function (e) {
    if (open && !inZone(e.target)) setOpen(false);
  });
  document.addEventListener('keydown', function (e) {
    if (e.key === 'Escape' && open) setOpen(false);
  });
  setOpen(false);
})();

  // ---- 表单提交防重复 + 按钮 loading（规范 8：提交中按钮 disabled）----
  (function () {
    'use strict';
    document.addEventListener('submit', function (e) {
      var form = e.target;
      if (!form || !form.method) return;
      if (form.method.toLowerCase() !== 'post') return;
      // 已被更早监听器拦截的提交不进入防重复态：文章保存/发布会先经确认对话框
      // （form 监听器 preventDefault，第二轮 requestSubmit 才真正提交）——若在此
      // 置 submitting + 禁用按钮，第二轮会被下方防重复误拦且 requestSubmit(禁用钮)
      // 被浏览器静默丢弃，保存/发布永远发不出去。
      if (e.defaultPrevented) return;
      // data-confirm 首轮：等待确认弹窗，不在此拦截（第二轮带 confirmed 才提交）
      if (form.hasAttribute('data-confirm') && !form.dataset.confirmed) return;
      if (form.dataset.submitting) { e.preventDefault(); return; }
      form.dataset.submitting = '1';
      var btn = e.submitter || form.querySelector('button[type="submit"]');
      if (btn && btn.classList.contains('btn')) btn.disabled = true;
    });
  })();

  // ---- 服务端 redirect ?msg=/?error=/?warn= → Toast（规范 7.11：操作结果统一反馈）----
  (function () {
    'use strict';
    var q = location.search;
    if (!q) return;
    var m = /[?&](msg|error|warn)=([^&#]*)/.exec(q);
    if (!m || !m[2]) return;
    var type = m[1] === 'error' ? 'error' : m[1] === 'warn' ? 'warn' : 'success';
    var text;
    try { text = decodeURIComponent(m[2].replace(/\+/g, ' ')); } catch (err) { text = m[2]; }
    if (!text) return;
    setTimeout(function () { window.hancicToast(text, type); }, 0);
  })();
