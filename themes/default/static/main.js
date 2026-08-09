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
    var images = document.querySelectorAll(".md-body img");
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
})();
