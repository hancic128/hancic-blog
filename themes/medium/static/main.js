/* 默认主题脚本：汉堡菜单、亮暗切换、配色选择、lightbox 与图片懒加载 */

(function () {
  "use strict";

  /* ---------- 亮暗切换（auto → light → dark → auto），默认 auto 跟随系统 ---------- */

  var THEME_KEY = "hancic-theme";
  var MODES = ["auto", "light", "dark"];

  function currentMode() {
    return document.documentElement.getAttribute("data-mode") || "auto";
  }

  function applyMode(mode) {
    document.documentElement.setAttribute("data-mode", mode);
    try { localStorage.setItem(THEME_KEY, mode); } catch (e) { /* 隐私模式忽略 */ }
  }

  // 有本地选择则覆盖服务端默认（无则保留 auto，跟随系统偏好）
  var savedMode;
  try { savedMode = localStorage.getItem(THEME_KEY); } catch (e) { savedMode = null; }
  if (savedMode && MODES.indexOf(savedMode) !== -1) {
    applyMode(savedMode);
  }

  var themeToggle = document.getElementById("theme-toggle");
  if (themeToggle) {
    themeToggle.addEventListener("click", function () {
      var next = MODES[(MODES.indexOf(currentMode()) + 1) % MODES.length];
      applyMode(next);
    });
  }

  /* ---------- 主题配色：调色板按钮弹出色板，点击色块切换（localStorage 记忆） ---------- */

  var ACCENT_KEY = "hancic-accent";
  var ACCENTS = ["indigo", "blue", "green", "purple", "orange"];

  function currentAccent() {
    return document.documentElement.getAttribute("data-accent") || "indigo";
  }

  function markCurrentSwatch() {
    var current = currentAccent();
    var swatches = document.querySelectorAll(".accent-swatch");
    for (var i = 0; i < swatches.length; i++) {
      swatches[i].classList.toggle("current", swatches[i].getAttribute("data-accent") === current);
    }
  }

  function applyAccent(accent) {
    document.documentElement.setAttribute("data-accent", accent);
    try { localStorage.setItem(ACCENT_KEY, accent); } catch (e) { /* 隐私模式忽略 */ }
    markCurrentSwatch();
  }

  // 恢复本地配色选择并标记当前色块
  var savedAccent;
  try { savedAccent = localStorage.getItem(ACCENT_KEY); } catch (e) { savedAccent = null; }
  if (savedAccent && ACCENTS.indexOf(savedAccent) !== -1) {
    applyAccent(savedAccent);
  } else {
    markCurrentSwatch();
  }

  var accentToggle = document.getElementById("accent-toggle");
  var accentPanel = document.getElementById("accent-panel");
  if (accentToggle && accentPanel) {
    accentToggle.addEventListener("click", function () {
      var willShow = accentPanel.hidden;
      accentPanel.hidden = !willShow;
      accentToggle.setAttribute("aria-expanded", willShow ? "true" : "false");
    });
    accentPanel.addEventListener("click", function (e) {
      var swatch = e.target.closest(".accent-swatch");
      if (!swatch) return;
      applyAccent(swatch.getAttribute("data-accent"));
      accentPanel.hidden = true;
      accentToggle.setAttribute("aria-expanded", "false");
    });
    // 点击页面其他区域关闭色板
    document.addEventListener("click", function (e) {
      if (!e.target.closest(".accent-wrap") && !accentPanel.hidden) {
        accentPanel.hidden = true;
        accentToggle.setAttribute("aria-expanded", "false");
      }
    });
  }

  /* ---------- 移动端汉堡菜单 ---------- */

  var navToggle = document.getElementById("nav-toggle");
  var siteNav = document.getElementById("site-nav");
  if (navToggle && siteNav) {
    navToggle.addEventListener("click", function () {
      siteNav.classList.toggle("open");
    });
    // 点击导航链接后收起菜单
    siteNav.addEventListener("click", function () {
      siteNav.classList.remove("open");
    });
  }

  /* ---------- 移动端下拉手风琴（≤768px）：点父项展开/收起，桌面 hover 不受影响 ---------- */
  var navDropdowns = document.querySelectorAll(".nav-dropdown");
  if (navDropdowns.length && window.matchMedia) {
    var isMobileNav = function () {
      return window.matchMedia("(max-width: 768px)").matches;
    };
    navDropdowns.forEach(function (dd) {
      var link = dd.querySelector(":scope > a");
      if (!link) return;
      link.addEventListener("click", function (e) {
        if (!isMobileNav()) return;
        var menu = dd.querySelector(".dropdown-menu");
        if (!menu) return;
        e.preventDefault();
        e.stopPropagation(); // 防止冒泡关闭抽屉
        dd.classList.toggle("open");
      });
    });
  }

  /* ---------- 图片懒加载 + lightbox ---------- */

  function addLazyAndLightbox() {
    // 正文图片与说说宫格图片共用同一套懒加载 + lightbox
    var images = document.querySelectorAll(".md-body img, .moment-grid img");
    for (var i = 0; i < images.length; i++) {
      var img = images[i];
      if (!img.hasAttribute("loading")) {
        img.setAttribute("loading", "lazy");
      }
      img.addEventListener("click", function (e) {
        e.preventDefault();
        openLightbox(this);
      });
    }
  }

  function openLightbox(img) {
    var overlay = document.createElement("div");
    overlay.className = "lightbox";
    var clone = img.cloneNode(false);
    overlay.appendChild(clone);
    document.body.appendChild(overlay);
    function close() {
      if (overlay.parentNode) {
        document.body.removeChild(overlay);
      }
      document.removeEventListener("keydown", onKey);
    }
    function onKey(e) {
      if (e.key === "Escape") {
        close();
      }
    }
    overlay.addEventListener("click", close);
    document.addEventListener("keydown", onKey);
  }

  // 懒加载：页面渲染后即可绑定（正文为服务端渲染，无动态插入）
  document.addEventListener("DOMContentLoaded", addLazyAndLightbox);
  if (document.readyState !== "loading") {
    addLazyAndLightbox();
  }

  /* ---------- 回到顶部按钮：滚动超过阈值才显示，点击平滑回顶 ---------- */

  function initBackTop() {
    var btn = document.getElementById("back-top");
    if (!btn) return;
    var THRESHOLD = 300;
    function onScroll() {
      var y = window.pageYOffset || document.documentElement.scrollTop || 0;
      btn.hidden = y <= THRESHOLD;
    }
    window.addEventListener("scroll", onScroll, { passive: true });
    onScroll();
    btn.addEventListener("click", function () {
      window.scrollTo({ top: 0, behavior: "smooth" });
    });
  }

  document.addEventListener("DOMContentLoaded", initBackTop);
  if (document.readyState !== "loading") {
    initBackTop();
  }

  /* ---------- 代码块复制按钮（正文 pre 右上角，点击复制代码） ---------- */

  function initCodeCopy() {
    var pres = document.querySelectorAll(".md-body pre");
    for (var i = 0; i < pres.length; i++) {
      var pre = pres[i];
      if (pre.querySelector(".code-copy")) continue;
      var btn = document.createElement("button");
      btn.type = "button";
      btn.className = "code-copy";
      btn.setAttribute("aria-label", "复制代码");
      btn.textContent = "复制";
      pre.appendChild(btn);
      btn.addEventListener("click", function () {
        var code = this.parentNode.querySelector("code");
        var text = code ? code.innerText : "";
        var done = function (ok) {
          this.textContent = ok ? "已复制" : "复制失败";
          this.classList.add("copied");
          var btn = this;
          setTimeout(function () {
            btn.textContent = "复制";
            btn.classList.remove("copied");
          }, 1500);
        }.bind(this);
        if (navigator.clipboard && window.isSecureContext) {
          navigator.clipboard.writeText(text).then(
            function () { done(true); },
            function () { fallbackCopy(text, done); }
          );
        } else {
          fallbackCopy(text, done);
        }
      });
    }
  }

  // 剪贴板 API 不可用时的降级复制（textarea + execCommand）
  function fallbackCopy(text, done) {
    try {
      var ta = document.createElement("textarea");
      ta.value = text;
      ta.style.position = "fixed";
      ta.style.opacity = "0";
      document.body.appendChild(ta);
      ta.select();
      var ok = document.execCommand("copy");
      document.body.removeChild(ta);
      done(ok);
    } catch (e) {
      done(false);
    }
  }

  document.addEventListener("DOMContentLoaded", initCodeCopy);
  if (document.readyState !== "loading") {
    initCodeCopy();
  }

  /* ---------- 代码块语法高亮（highlight.js）+ 语言标记；mermaid 渲染为图 ---------- */
  function renderMermaid(pre, code) {
    if (!window.mermaid || !mermaid.render) return;
    var text = (code.innerText || code.textContent || "").trim();
    if (!text) return;
    var holder = document.createElement("div");
    holder.className = "mermaid-chart";
    var theme = document.documentElement.getAttribute("data-mode") === "dark" ? "dark" : "default";
    mermaid.initialize({ startOnLoad: false, theme: theme });
    var pid = "hancic-mmd-" + Math.floor(Math.random() * 1e9);
    mermaid.render(pid, text).then(function (res) {
      holder.innerHTML = res.svg;
      pre.replaceWith(holder);
    }).catch(function () {
      holder.className += " mermaid-chart-error";
      holder.textContent = "mermaid 渲染失败：请检查语法";
      pre.replaceWith(holder);
    });
  }

  function initCodeHighlight() {
    if (!window.hljs || !hljs.highlightElement) return;
    var codes = document.querySelectorAll(".md-body pre code");
    for (var i = 0; i < codes.length; i++) {
      var code = codes[i];
      var pre = code.closest("pre");
      var m = (code.className || "").match(/language-([\w+-]+)/);
      var lang = m ? m[1] : "";
      if (lang === "mermaid") {
        renderMermaid(pre, code);
        continue;
      }
      try {
        hljs.highlightElement(code);
      } catch (e) { /* 单块失败不影响其它 */ }
      if (lang && pre) pre.setAttribute("data-language", lang);
    }
  }
  document.addEventListener("DOMContentLoaded", initCodeHighlight);
  if (document.readyState !== "loading") {
    initCodeHighlight();
  }

  /* ---------- 首页文章列表滚动浮现（淡入 + 上移 8px，300ms ease-out） ---------- */

  function initScrollReveal() {
    var items = document.querySelectorAll(".post-list-item");
    if (items.length === 0) return;
    // 无 IntersectionObserver 或用户偏好减弱动效时不加类，内容保持默认可见
    if (!("IntersectionObserver" in window)) return;
    if (window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;

    for (var i = 0; i < items.length; i++) {
      items[i].classList.add("reveal");
    }
    var observer = new IntersectionObserver(function (entries) {
      for (var j = 0; j < entries.length; j++) {
        if (entries[j].isIntersecting) {
          entries[j].target.classList.add("is-visible");
          observer.unobserve(entries[j].target);
        }
      }
    }, { rootMargin: "0px 0px -8% 0px", threshold: 0.1 });
    for (var k = 0; k < items.length; k++) {
      observer.observe(items[k]);
    }
  }

  document.addEventListener("DOMContentLoaded", initScrollReveal);
  if (document.readyState !== "loading") {
    initScrollReveal();
  }
})();

// 发布日历 tooltip：body 级悬浮层（避免被热力图滚动容器裁剪），跟随鼠标
(function initHeatTooltip() {
  const tip = document.createElement("div");
  tip.className = "heat-tooltip";
  tip.style.display = "none";
  document.body.appendChild(tip);
  const show = (cell, e) => {
    tip.textContent = cell.getAttribute("data-tip") || "";
    tip.style.display = "block";
    move(cell, e);
  };
  const move = (_cell, e) => {
    const w = tip.offsetWidth || 220;
    let x = e.clientX + 14;
    if (x + w > window.innerWidth - 8) x = e.clientX - w - 14;
    tip.style.left = x + "px";
    tip.style.top = (e.clientY + 14) + "px";
  };
  const hide = () => { tip.style.display = "none"; };
  const on = (cell) => {
    cell.addEventListener("mouseenter", (e) => show(cell, e));
    cell.addEventListener("mousemove", (e) => move(cell, e));
    cell.addEventListener("mouseleave", hide);
    // 触屏：点击切换
    cell.addEventListener("click", (e) => {
      e.stopPropagation();
      tip.style.display === "block" ? hide() : show(cell, e);
    });
  };
  document.querySelectorAll(".heat-cell").forEach(on);
})();

// 最近说说折叠/展开：点击切换 .expanded（不依赖 details 原生行为）。
// 首页与说说页同款：默认折叠（箭头 ▸），点击展开全文（箭头 ▾）。
// 内容短的说说（预览未截断，即全文 ≤40 字）不需要折叠：直接显示全文、隐藏 toggle。
(function initMomentToggle() {
  document.querySelectorAll(".moment-item").forEach((item) => {
    const preview = item.querySelector(".moment-preview");
    const full = item.querySelector(".moment-full");
    const toggle = item.querySelector(".moment-toggle");
    if (preview && full && toggle) {
      const short = preview.textContent.trim() === full.textContent.trim();
      item.classList.toggle("moment-short", short);
      if (short) {
        toggle.style.display = "none";
        item.querySelector(".moment-body")?.classList.add("expanded");
        return;
      }
    }
    if (!toggle) return;
    toggle.addEventListener("click", () => {
      const body = toggle.closest(".moment-body");
      const expanded = body.classList.toggle("expanded");
      toggle.setAttribute("aria-expanded", expanded ? "true" : "false");
      const arrow = toggle.querySelector(".moment-arrow");
      if (arrow) arrow.textContent = expanded ? "▾" : "▸";
    });
  });
})();

// 归档页侧栏月份筛选：默认显示最近半年（6 个月份），点击"显示更多月份"展开其余
(function initMonthMore() {
  const items = document.querySelectorAll("#side-months li");
  const btn = document.getElementById("month-more-btn");
  if (!items.length || !btn) return;
  const hidden = [];
  for (let i = 6; i < items.length; i++) {
    items[i].style.display = "none";
    hidden.push(items[i]);
  }
  btn.addEventListener("click", () => {
    const expanding = btn.getAttribute("aria-expanded") !== "true";
    for (const li of hidden) li.style.display = expanding ? "" : "none";
    btn.setAttribute("aria-expanded", expanding ? "true" : "false");
    btn.textContent = expanding ? "收起 ▴" : "显示更多月份 ▾";
  });
})();

// 悬浮联系方式卡片：无内容时隐藏按钮；点击展开/收起，Esc 或点击外部关闭
(function initContactFab() {
  const fab = document.getElementById("contact-fab");
  const panel = document.getElementById("contact-panel");
  if (!fab || !panel) return;
  const hasContent = panel.querySelector(".contact-item");
  if (!hasContent) return; // 未配置任何联系方式：保持 hidden 不展示
  fab.hidden = false;
  const close = () => {
    panel.hidden = true;
    fab.setAttribute("aria-expanded", "false");
  };
  fab.addEventListener("click", () => {
    const willShow = panel.hidden;
    panel.hidden = !willShow;
    fab.setAttribute("aria-expanded", willShow ? "true" : "false");
  });
  document.addEventListener("click", (e) => {
    if (!e.target.closest("#contact-panel, #contact-fab") && !panel.hidden) close();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !panel.hidden) close();
  });
})();

// 列表页排序切换：保留当前 URL 其余参数，仅替换 sort
(function initSortBar() {
  const bars = document.querySelectorAll(".sort-bar");
  if (!bars.length) return;
  bars.forEach((bar) => {
    bar.querySelectorAll(".sort-link").forEach((a) => {
      a.addEventListener("click", (e) => {
        e.preventDefault();
        const params = new URLSearchParams(window.location.search);
        params.set("sort", a.getAttribute("data-sort"));
        params.delete("page");
        const qs = params.toString();
        window.location.href = window.location.pathname + (qs ? "?" + qs : "");
      });
    });
  });
})();

// 标签过多折叠：超过 MAX 个隐藏并追加 "+N" 展开按钮
(function initTagFold() {
  const MAX = 5;
  const TAG_CLOUD_MAX = 15;
  const fold = (container, links, limit) => {
    if (links.length <= limit) return;
    for (let i = limit; i < links.length; i++) links[i].style.display = "none";
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "tag-more";
    btn.textContent = "+" + (links.length - limit);
    btn.setAttribute("aria-expanded", "false");
    container.appendChild(btn);
    btn.addEventListener("click", () => {
      const expanded = btn.getAttribute("aria-expanded") === "true";
      for (let i = limit; i < links.length; i++) links[i].style.display = expanded ? "none" : "";
      btn.setAttribute("aria-expanded", expanded ? "false" : "true");
      btn.textContent = expanded ? "+" + (links.length - limit) : "收起";
    });
  };
  document.querySelectorAll(".post-tags").forEach((box) => fold(box, box.querySelectorAll(".tag-link"), MAX));
  const meta = document.querySelector(".post-header .meta");
  if (meta) fold(meta, meta.querySelectorAll('a[href^="/tag/"]'), MAX);
  document.querySelectorAll(".tag-cloud").forEach((box) => fold(box, box.querySelectorAll(".tag-badge"), TAG_CLOUD_MAX));
})();

// 阅读模式：切换 body.reading-mode 隐藏站点导航/侧栏/页脚，状态记 localStorage
(function initReadingMode() {
  const btn = document.querySelector("[data-reading-toggle]");
  if (!btn) return; // 仅文章页/独立页存在
  const apply = (on) => {
    document.body.classList.toggle("reading-mode", on);
    btn.setAttribute("aria-pressed", on ? "true" : "false");
    try { localStorage.setItem("reading-mode", on ? "1" : "0"); } catch (e) {}
  };
  btn.addEventListener("click", () => apply(!document.body.classList.contains("reading-mode")));
  let saved = null;
  try { saved = localStorage.getItem("reading-mode"); } catch (e) {}
  if (saved === "1") apply(true);
})();

/* ---------- 文章页复制全文（Markdown 原文，方便发布到其他平台） ---------- */

function showToast(msg) {
  var t = document.createElement("div");
  t.className = "site-toast";
  t.textContent = msg;
  document.body.appendChild(t);
  requestAnimationFrame(function () { t.classList.add("show"); });
  setTimeout(function () {
    t.classList.remove("show");
    setTimeout(function () { if (t.parentNode) t.parentNode.removeChild(t); }, 300);
  }, 1800);
}

function initCopyFull() {
  var btns = document.querySelectorAll(".copy-full[data-copy-md]");
  if (!btns.length) return;
  for (var i = 0; i < btns.length; i++) {
    (function (btn) {
      var md = btn.getAttribute("data-copy-md");
      var label = btn.querySelector(".copy-full-label");
      var what = label ? label.textContent : "内容";
      btn.addEventListener("click", function () {
        var done = function (ok) {
          if (ok) {
            btn.classList.add("copied");
            showToast("已复制" + what);
            setTimeout(function () { btn.classList.remove("copied"); }, 450);
          } else {
            showToast("复制失败");
          }
        };
        if (navigator.clipboard && window.isSecureContext) {
          navigator.clipboard.writeText(md).then(
            function () { done(true); },
            function () { fallbackCopy(md, done); }
          );
        } else {
          fallbackCopy(md, done);
        }
      });
    })(btns[i]);
  }
}

document.addEventListener("DOMContentLoaded", initCopyFull);
if (document.readyState !== "loading") {
  initCopyFull();
}

/* 前台工具悬浮组（与后台一致）：hover 设备鼠标移到主按钮上才展开、移出延迟收起。
   折叠时列表不占位（hidden），触发区域即主按钮，避免空白区域误展开 */
(function () {
  "use strict";
  var root = document.getElementById("front-fab");
  var main = document.getElementById("front-fab-main");
  var accentPanel = document.getElementById("accent-panel");
  var list = root ? root.querySelector(".front-fab-list") : null;
  if (!root || !main || !list) return;
  var open = false;
  var closeTimer = null;
  var hideTimer = null;
  var placeTimer = null;
  var GAP = 14;
  var BTN_GAP = 12;

  list.classList.add("hidden"); // 初始折叠：列表不占位，hover 区域 = 主按钮

  function firstVisibleItemTop() {
    var items = root.querySelectorAll(".front-fab-item");
    for (var i = 0; i < items.length; i++) {
      var btn = items[i].querySelector(".front-fab-btn");
      if (btn && !btn.hidden) return items[i].getBoundingClientRect().top;
    }
    return null;
  }
  function layoutFloats(expanded) {
    var back = document.getElementById("back-top");
    var read = document.querySelector(".reading-float");
    if (!back && !read) return;
    var base = 90;
    if (expanded) {
      var top = firstVisibleItemTop();
      if (top !== null) base = window.innerHeight - top + GAP;
    }
    var backOn = !!(back && !back.hidden);
    var readOn = !!read;
    if (backOn && readOn) {
      back.style.bottom = base + "px";
      read.style.bottom = (base + 42 + BTN_GAP) + "px";
    } else if (backOn) {
      back.style.bottom = base + "px";
      if (read) read.style.bottom = "";
    } else if (readOn) {
      read.style.bottom = base + "px";
      if (back) back.style.bottom = "";
    } else {
      if (back) back.style.bottom = "";
      if (read) read.style.bottom = "";
    }
  }
  function setOpen(v) {
    open = v;
    if (placeTimer) { clearTimeout(placeTimer); placeTimer = null; }
    if (v) {
      if (hideTimer) { clearTimeout(hideTimer); hideTimer = null; }
      list.classList.remove("hidden");
      // 下一帧再加展开类：让 display 生效后再播放入场动画
      requestAnimationFrame(function () {
        requestAnimationFrame(function () {
          root.classList.add("fab-open");
          main.setAttribute("aria-expanded", "true");
        });
      });
      placeTimer = setTimeout(function () { layoutFloats(true); }, 260);
    } else {
      root.classList.remove("fab-open");
      main.setAttribute("aria-expanded", "false");
      layoutFloats(false);
      // 收起动画结束后再隐藏列表，让折叠后的 hover 区域回到主按钮
      hideTimer = setTimeout(function () { list.classList.add("hidden"); }, 240);
    }
  }
  function cancelClose() {
    if (closeTimer) { clearTimeout(closeTimer); closeTimer = null; }
  }
  function scheduleClose() {
    cancelClose();
    closeTimer = setTimeout(function () { setOpen(false); }, 250);
  }
  // hover 设备：只把鼠标放到可见组件上才触发 —— 折叠时可视区即主按钮；
  // 展开后保持在列表内则不收起；色板 hover 同样保持
  if (window.matchMedia("(hover: hover)").matches) {
    main.addEventListener("mouseenter", function () { cancelClose(); if (!open) setOpen(true); });
    root.addEventListener("mouseenter", function () { if (open) cancelClose(); });
    root.addEventListener("mouseleave", function () { if (open) scheduleClose(); });
    main.addEventListener("mouseleave", function () { if (open && !root.matches(":hover")) scheduleClose(); });
    if (accentPanel) {
      accentPanel.addEventListener("mouseenter", cancelClose);
      accentPanel.addEventListener("mouseleave", scheduleClose);
    }
  }
  main.addEventListener("click", function (e) {
    e.stopPropagation();
    setOpen(!open);
  });
  root.addEventListener("click", function (e) {
    if (e.target.closest("#contact-fab")) setOpen(false);
  });
  document.addEventListener("click", function (e) {
    if (open && !e.target.closest(".front-fab") && !e.target.closest("#accent-panel")) setOpen(false);
  });
  document.addEventListener("keydown", function (e) {
    if (e.key === "Escape" && open) setOpen(false);
  });
  var scrollT = null;
  function onViewportChange() {
    if (scrollT) return;
    scrollT = setTimeout(function () {
      scrollT = null;
      if (open) setOpen(true); else layoutFloats(false);
    }, 120);
  }
  window.addEventListener("scroll", onViewportChange, { passive: true });
  window.addEventListener("resize", onViewportChange);
  layoutFloats(false);
})();
