import { test, expect } from "@playwright/test";
import { loginAsAdmin } from "../helpers";

test("后台侧栏折叠按钮位于侧栏内且可折叠", async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto("/admin");

  const button = page.locator("#admin-side #side-collapse");
  await expect(button).toHaveCount(1);
  await expect(page.locator("#fab-group #side-collapse")).toHaveCount(0);

  const shell = page.locator("#admin-shell");
  await expect(shell).not.toHaveClass(/side-collapsed/);
  await button.click();
  await expect(shell).toHaveClass(/side-collapsed/);
  await button.click();
  await expect(shell).not.toHaveClass(/side-collapsed/);
});

test("帮助页采用左右两列居中布局且目录不遮挡正文", async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto("/admin/help");

  // 1) 目录可见
  const toc = page.locator(".api-toc");
  await expect(toc).toBeVisible();

  // 2) 正文容器是水平居中（margin-left ≈ margin-right），与其它后台页面一致
  const contentBox = await page.locator(".admin-content").boundingBox();
  const shellBox = await page.locator(".admin-main").boundingBox();
  expect(contentBox).not.toBeNull();
  expect(shellBox).not.toBeNull();
  if (contentBox && shellBox) {
    const leftGap = contentBox.x - shellBox.x;
    const rightGap = shellBox.x + shellBox.width - (contentBox.x + contentBox.width);
    // 帮助页正文自身有 max-width 限宽；两边的留白差 < 4px 表示水平居中
    expect(Math.abs(leftGap - rightGap)).toBeLessThan(4);
  }

  // 3) 目录不再压在正文中：toc.x < panel.x + panel.width（说明 toc 在左、正文在右）
  const tocBox = await toc.boundingBox();
  const sectionBox = await page.locator(".panel-section").first().boundingBox();
  expect(tocBox).not.toBeNull();
  expect(sectionBox).not.toBeNull();
  if (tocBox && sectionBox) {
    // 目录在正文左侧（toc.right ≤ section.x + 一点点间隙）
    expect(tocBox.x + tocBox.width).toBeLessThanOrEqual(sectionBox.x + 40);
  }
});

test("品牌区点击新标签页打开博客首页；侧栏底部显示版本徽章与最近部署时间", async ({ page, context }) => {
  await loginAsAdmin(page);
  await page.goto("/admin");

  // 侧栏底部 admin-meta：版本徽章 + 部署时间
  const meta = page.locator(".admin-side .admin-meta");
  await expect(meta).toBeVisible();

  const versionTag = meta.locator(".admin-version-tag");
  await expect(versionTag).toBeVisible();
  // 版本徽章形如 vX.Y.Z（CARGO_PKG_VERSION 前缀 v）
  await expect(versionTag).toHaveText(/^v\d+\.\d+\.\d+$/);

  const deploy = meta.locator(".admin-deploy");
  await expect(deploy).toBeVisible();
  // 部署时间形如「YYYY-MM-DD 部署」，去掉时分秒以适配侧栏底部窄列
  await expect(deploy).toHaveText(/^最近部署 \d{4}-\d{2}-\d{2} 部署$/);

  // 品牌区 hover 不出现下划线（覆盖全局 a:hover { text-decoration: underline }）
  const brand = page.locator("a.admin-brand");
  await expect(brand).toHaveAttribute("href", "/");
  await expect(brand).toHaveAttribute("target", "_blank");
  await brand.hover();
  const td = await brand.evaluate((el) => getComputedStyle(el).textDecorationLine);
  expect(td).toBe("none");

  // 品牌区为新标签页链接：点击后在新增的 page 上落到博客首页
  const [opened] = await Promise.all([context.waitForEvent("page"), brand.click()]);
  await opened.waitForLoadState("domcontentloaded");
  expect(new URL(opened.url()).pathname).toBe("/");
});

test("切换菜单加载动画：loading-bar 元素存在且未加载时透明度为 0", async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto("/admin");

  // 1) 进度条容器存在（body 下第一个元素）
  const bar = page.locator(".admin-loading-bar");
  await expect(bar).toHaveCount(1);

  // 2) 未加载时进度条 ::before 不显示（opacity = 0，CSS 控制）
  //    （用 .is-loading 切换显示；默认无 is-loading 时透明）
  const opacity = await bar.evaluate(
    (el) => getComputedStyle(el, "::before").opacity,
  );
  expect(opacity).toBe("0");

  // 3) CSS 动画名定义存在——确认 keyframes 生效
  const animationName = await bar.evaluate(
    (el) => getComputedStyle(el, "::before").animationName,
  );
  expect(animationName).toBe("admin-loading-slide");
});

test("后台内容区在宽屏下自适应变宽（父容器限宽时取 min，不超过 1600）", async ({ page }) => {
  await loginAsAdmin(page);

  // 默认 desktop 视口 1280：内容区宽度受侧栏 236px + 内容 padding 限制
  // （关键修复前是固定 max-width: 1120px，1280 视口下被父容器卡到 ≤1044；
  // 现在改为 clamp，父容器宽度足够时能真正撑到 1120+）
  await page.goto("/admin");
  const w1280 = await page.locator(".admin-content").evaluate(
    (el) => el.getBoundingClientRect().width,
  );
  // 不超过视口（去滚动条）和父容器（侧栏 236 + padding）
  expect(w1280).toBeLessThanOrEqual(1280 - 236);

  // 拉到 1920：内容区应随之拉宽（70vw = 1344，clamp 上限 1600）
  await page.setViewportSize({ width: 1920, height: 1080 });
  await page.reload();
  const w1920 = await page.locator(".admin-content").evaluate(
    (el) => el.getBoundingClientRect().width,
  );
  // 关键断言：宽屏比窄屏更宽
  expect(w1920).toBeGreaterThan(w1280 + 100);
  // clamp 上限 1600，且有左右 padding——实测应 < 1600
  expect(w1920).toBeLessThanOrEqual(1600);
});

