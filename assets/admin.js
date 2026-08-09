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
        if (e.submitter && e.submitter.dataset && e.submitter.dataset.action) {
          statusInput.value = e.submitter.dataset.action;
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
      cache: isNewPost ? { enable: true, id: DRAFT_KEY } : false,
      height: 460,
      value: editorEl.getAttribute('data-content') || '',
      after: function () {
        lastSaved = editor.getValue();
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

