import { test, expect, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";
import path from "node:path";
import { loginAsAdmin, pastePngToEditor } from "../helpers";

const FIXTURE = path.resolve(__dirname, "../fixtures/1x1.png");
const PNG_B64 = readFileSync(FIXTURE).toString("base64");

/** 附件库当前附件卡片数（空库无 .att-grid，直接数 .att-card 即可） */
async function attachmentCount(page: Page): Promise<number> {
  await page.goto("/admin/attachments");
  await page.waitForLoadState("load");
  return page.locator(".att-card").count();
}

/**
 * 粘贴上传（桌面视口）：
 * 编辑页 → 构造 ClipboardEvent（clipboardData 含 PNG File）dispatch 到
 * milkdown WYSIWYG 编辑器 → 真实触发上传（POST /api/uploads）
 * → 编辑器出现 <img src="/uploads/..."> 且附件库 +1。
 */
test("粘贴图片上传：编辑器出现 uploads 图片且附件库 +1", async ({ page }) => {
  await loginAsAdmin(page);

  const before = await attachmentCount(page);

  // 编辑页等 milkdown 就绪
  await page.goto("/admin/posts/new");
  await page.locator("#editor .ProseMirror").waitFor({ state: "visible" });

  // 捕获上传请求：能等到该请求说明确实触发了 paste 上传
  const uploadRes = page.waitForResponse(
    (r) => r.request().method() === "POST" && r.url().includes("/api/uploads"),
    { timeout: 15_000 },
  );

  // 构造 ClipboardEvent（ClipboardEvent 的 clipboardData 由 DataTransfer 提供）
  await pastePngToEditor(page, PNG_B64);

  await uploadRes;

  // milkdown 插入 markdown 图片：编辑器里出现 /uploads/ 图片
  const editorImg = page.locator('#editor img[src^="/uploads/"]').first();
  await expect(editorImg).toBeVisible({ timeout: 15_000 });
  const src = await editorImg.getAttribute("src");
  expect(src).toMatch(/^\/uploads\//);

  // 附件库 +1
  const after = await attachmentCount(page);
  expect(after).toBe(before + 1);
});
