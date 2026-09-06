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
  // 上传成功会 302 到 /admin/trails?msg=成功导入…（URL 带 query，glob 需放宽到路径前缀）
  await page.waitForURL("**/admin/trails?**");
  await expect(page.locator(".trail-admin-link").first()).toBeVisible();
  await expect(page.locator(".trail-admin-link").first()).toContainText("E2E 测试轨迹");

  // ---- 前台总览：地图 + 轨迹卡片 ----
  // CI/外网瓦片挂起会使 load 事件延迟：页面功能由 Leaflet 容器类断言，等 DOM 即可
  await page.goto("/trails", { waitUntil: "domcontentloaded" });
  const mapEl = page.locator("#trails-map");
  await expect(mapEl).toBeVisible();
  // Leaflet 初始化后容器自身带 .leaflet-container（瓦片为外网资源，不断言瓦片加载）
  await expect(mapEl).toHaveClass(/leaflet-container/);
  const card = page.locator(".trail-card").first();
  await expect(card).toBeVisible();
  await expect(page.locator(".trail-card-name").first()).toContainText("E2E 测试轨迹");

  // ---- 前台总览：默认选中聚焦首条（a586d9c，详情面板显示），详情页独立路由 ----
  const detailUrl = await card.getAttribute("href");
  // 默认选中聚焦由 selectTrail 完成：面板出现且名称已填充（轮询等待地图/JS 时序）
  await expect(page.locator("#trail-detail")).not.toHaveAttribute("hidden", "", { timeout: 10_000 });
  await expect(page.locator("#trail-detail-name")).toContainText("E2E 测试轨迹");

  // ---- 详情页：地图 + 数据卡片 + 瓦片图层切换控件 ----
  await page.goto(detailUrl!, { waitUntil: "domcontentloaded" });
  await expect(page.locator("#trail-map")).toHaveClass(/leaflet-container/);
  await expect(page.locator(".trail-stats .trail-stat").first()).toBeVisible();
  await expect(page.locator(".trail-stat-value").first()).toBeVisible();
  await expect(page.locator(".leaflet-control-layers")).toBeVisible();
});
