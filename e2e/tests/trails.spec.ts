import { test, expect } from "@playwright/test";
import path from "node:path";
import { loginAsAdmin } from "../helpers";

/** fixture GPX 绝对路径（metadata name = E2E 测试轨迹） */
const GPX = path.resolve(__dirname, "../fixtures/trail.gpx");

/**
 * 徒步轨迹全流程（桌面视口）：
 * 登录 → 后台 /admin/trails 上传 GPX → 列表出现轨迹 →
 * 前台 /trails 总览（地图容器 + 轨迹卡片）→ 详情页（地图 + 数据卡片 + 瓦片图层切换控件）。
 */
test("轨迹全流程：后台上传 → 前台总览与详情显示", async ({ page }) => {
  await loginAsAdmin(page);

  // ---- 后台上传 GPX（普通 multipart 表单）----
  await page.goto("/admin/trails");
  await page.locator('input[type="file"][name="file"]').setInputFiles(GPX);
  const uploadRes = page.waitForResponse(
    (r) =>
      r.request().method() === "POST" &&
      /\/admin\/trails\/upload$/.test(r.request().url()),
  );
  await page.locator('.trail-upload-form button[type="submit"]').click();
  await uploadRes;
  await page.waitForURL("**/admin/trails");
  await expect(page.locator(".trail-admin-link").first()).toBeVisible();
  await expect(page.locator(".trail-admin-link").first()).toContainText("E2E 测试轨迹");

  // ---- 前台总览：地图 + 轨迹卡片 ----
  await page.goto("/trails");
  const mapEl = page.locator("#trails-map");
  await expect(mapEl).toBeVisible();
  // Leaflet 初始化后容器自身带 .leaflet-container（瓦片为外网资源，不断言瓦片加载）
  await expect(mapEl).toHaveClass(/leaflet-container/);
  const card = page.locator(".trail-card").first();
  await expect(card).toBeVisible();
  await expect(page.locator(".trail-card-name").first()).toContainText("E2E 测试轨迹");

  // ---- 详情页：地图 + 数据卡片 + 瓦片图层切换控件 ----
  // 卡片位于地图下方：先滚动到视口再点击（地图 tile 层已 overflow:hidden 裁切）
  await card.scrollIntoViewIfNeeded();
  await card.click();
  await page.waitForURL("**/trails/*");
  await expect(page.locator("#trail-map")).toHaveClass(/leaflet-container/);
  await expect(page.locator(".trail-stats .trail-stat").first()).toBeVisible();
  await expect(page.locator(".trail-stat-value").first()).toBeVisible();
  await expect(page.locator(".leaflet-control-layers")).toBeVisible();
});
