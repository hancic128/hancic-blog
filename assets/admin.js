// 后台全局脚本：CSRF 注入、data-confirm 确认弹窗、抽屉导航（≤768px）与仪表盘趋势图。
(function () {
  'use strict';

  // ---- CSRF：读取 meta，注入 POST 表单；fetch 包装自动带 X-CSRF-Token ----
  var csrfMeta = document.querySelector('meta[name="csrf-token"]');
  var csrfToken = csrfMeta ? csrfMeta.getAttribute('content') : '';

  document.addEventListener('submit', function (e) {
    var form = e.target;
    if (!form || form.method === undefined) return;
    if (form.method.toLowerCase() !== 'post') return;
    // data-confirm 确认弹窗（取消则阻止提交）
    if (form.hasAttribute('data-confirm') &&
        !window.confirm(form.getAttribute('data-confirm'))) {
      e.preventDefault();
      return;
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

  // ---- data-confirm：链接点击确认（表单提交在 submit 事件里处理）----
  document.addEventListener('click', function (e) {
    var el = e.target instanceof Element ? e.target.closest('[data-confirm]') : null;
    if (!el || el.tagName !== 'A') return;
    if (!window.confirm(el.getAttribute('data-confirm'))) {
      e.preventDefault();
    }
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
    new window.Chart(canvas, {
      type: 'line',
      data: {
        labels: window.chartData.labels,
        datasets: [{
          label: '阅读量',
          data: window.chartData.data,
          borderColor: '#3d6df0',
          backgroundColor: 'rgba(61, 109, 240, 0.12)',
          fill: true,
          tension: 0.3,
          pointRadius: 2
        }]
      },
      options: {
        responsive: true,
        plugins: { legend: { display: false } },
        scales: { y: { beginAtZero: true, ticks: { precision: 0 } } }
      }
    });
  });
})();

// ---- 文章编辑器（Vditor IR）与自动保存 ----
(function () {
  'use strict';

  var editorEl = document.getElementById('editor');
  if (!editorEl || !window.Vditor) return;

  var form = document.getElementById('post-form');
  var statusInput = document.getElementById('post-status');
  var statusEl = document.getElementById('save-status');
  var editor = null;
  var lastSaved = null;

  // Vditor 上传处理器：files → POST /api/uploads（hancicFetch 自动带 CSRF 头）
  // → 组装 Vditor 期望的 { msg, code, data: { errFiles, succMap } }。
  window.vditorUpload = function (files) {
    var data = new FormData();
    files.forEach(function (file) { data.append('files', file); });
    return window.hancicFetch('/api/uploads', { method: 'POST', body: data })
      .then(function (res) { return res.json(); })
      .then(function (json) {
        var succMap = {};
        (json.data || []).forEach(function (att) {
          succMap[att.orig_name] = '/uploads/' + att.path;
        });
        return { msg: '', code: 0, data: { errFiles: [], succMap: succMap } };
      })
      .catch(function () {
        return { msg: '上传失败，请重试', code: 1, data: { errFiles: [], succMap: {} } };
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

  function autosave() {
    if (!editor || !window._post || !window._post.id) return;
    if (!contentChanged()) return;
    var data = new FormData();
    data.append('content_md', currentContent());
    window.hancicFetch('/admin/posts/' + window._post.id + '/autosave', {
      method: 'POST',
      body: data
    })
      .then(function (res) { return res.ok ? res.json() : Promise.reject(res); })
      .then(function () {
        lastSaved = currentContent();
        showSaved();
      })
      .catch(function () { /* 静默失败：下一次间隔或 pagehide 重试 */ });
  }

  // 存草稿 / 发布：点击的按钮 data-action 写入隐藏 status 再随表单提交
  if (form) {
    form.addEventListener('submit', function (e) {
      if (e.submitter && e.submitter.dataset && e.submitter.dataset.action) {
        statusInput.value = e.submitter.dataset.action;
      }
    });
  }

  // 初始化 Vditor：IR 模式，内容取自容器 data-content（页面已 tera 转义）
  editor = new window.Vditor('editor', {
    mode: 'ir',
    cache: false,
    height: 460,
    value: editorEl.getAttribute('data-content') || '',
    after: function () { lastSaved = editor.getValue(); },
    upload: { handler: window.vditorUpload }
  });

  // 自动保存：30s 轮询 + pagehide 兜底（仅内容变化时发请求）
  setInterval(autosave, 30000);
  window.addEventListener('pagehide', function () {
    if (contentChanged()) autosave();
  });
})();

