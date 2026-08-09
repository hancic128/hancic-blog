/* 默认主题脚本：汉堡菜单、亮暗色切换、lightbox 与图片懒加载 */

(function () {
  "use strict";

  var THEME_KEY = "hancic-theme";
  var MODES = ["auto", "light", "dark"];

  /* ---------- 亮暗色切换（auto → light → dark → auto） ---------- */

  function currentMode() {
    return document.documentElement.getAttribute("data-mode") || "auto";
  }

  function applyMode(mode) {
    document.documentElement.setAttribute("data-mode", mode);
    try { localStorage.setItem(THEME_KEY, mode); } catch (e) { /* 隐私模式忽略 */ }
  }

  // 有本地选择则覆盖服务端默认（无则保留 auto，跟随系统偏好）
  var saved;
  try { saved = localStorage.getItem(THEME_KEY); } catch (e) { saved = null; }
  if (saved && MODES.indexOf(saved) !== -1) {
    applyMode(saved);
  }

  var themeToggle = document.getElementById("theme-toggle");
  if (themeToggle) {
    themeToggle.addEventListener("click", function () {
      var next = MODES[(MODES.indexOf(currentMode()) + 1) % MODES.length];
      applyMode(next);
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
})();
