import { test, expect } from "@playwright/test";
import path from "node:path";
import { loginAsAdmin } from "../helpers";

/** fixture 1x1 PNG 绝对路径 */
const FIXTURE = path.resolve(__dirname, "../fixtures/1x1.png");

/**
 * 说说发布流程（桌面视口）：
 * 登录 → 后台说说 → 输入文字 + 选图上传（setInputFiles）→ 发布 →
 * 前台 /moments 可见文字与 <img>。
 */
test("发布带图说说：后台上传图 → 前台可见文字与图片", async ({ page }) => {
  const text = `E2E 说说 ${Date.now()} e2e-moment-text 这是一条足够长的说说正文内容，确保超过四十字的预览截断阈值从而启用折叠展开功能`;

  await loginAsAdmin(page);

  // 后台说说页
  await page.goto("/admin/moments");
  await page.locator("#moment-content").fill(text);

  // 选图上传（隐藏 file input，setInputFiles 直接可用）→ 等待缩略图预览出现
  await page.locator("#moment-file").setInputFiles(FIXTURE);
  const pickImg = page.locator(".moment-pick img");
  await expect(pickImg).toBeVisible({ timeout: 15_000 });

  // 发布：POST /admin/moments → 302 回 /admin/moments
  const createRes = page.waitForResponse(
    (r) => r.request().method() === "POST" && /\/admin\/moments$/.test(r.request().url()),
  );
  await page.locator('#moment-form button[type="submit"]').click();
  await createRes;
  await page.waitForURL("**/admin/moments");

  // 后台列表应出现该说说
  await expect(page.locator(".moment-admin-content").filter({ hasText: text })).toBeVisible();

  // 前台 /moments：时间线默认折叠，先展开该条再说
  await page.goto("/moments");
  const body = page
    .locator(".moment-body")
    .filter({ has: page.locator(`text=${text}`) })
    .first();
  await body.locator(".moment-toggle").click();
  const moment = page.locator(".moment-content").filter({ hasText: text });
  await expect(moment).toBeVisible();
  // 同一说说卡片内的图片（展开态完整宫格；折叠态缩略图不参与断言）
  const card = page.locator(".moment-item").filter({ has: page.locator(`text=${text}`) });
  await expect(card.locator(".moment-grid img").first()).toBeVisible({
    timeout: 15_000,
  });
});
