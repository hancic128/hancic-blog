import { test, expect } from "@playwright/test";
import { loginAsAdmin, acceptConfirmDialog, clearEditor } from "../helpers";

/**
 * 文章发布全流程（桌面视口）：
 * 登录 → 新建文章 → Vditor 输入标题/正文 → 存草稿 → 发布 →
 * 前台 /post/{slug} 可见正文与阅读量。
 */
test("发布文章：新建→草稿→发布→前台可见正文", async ({ page }) => {
  const title = `E2E 文章 ${Date.now()}`;
  const body = "这是端到端测试正文内容 e2e-body-text";

  await loginAsAdmin(page);

  // 新建文章（固定链接由系统生成短 uuid，无需填写）
  await page.goto("/admin/posts/new");
  await page.locator("#post-title").fill(title);
  // 等 Vditor 挂载完成（CDN 加载后）
  await page.locator("#editor .ProseMirror").waitFor({ state: "visible" });
  await page.locator("#editor .ProseMirror").click();
  // 清空新建页预填的 Markdown 语法模板
  await clearEditor(page);
  await page.keyboard.type(body);

  // 存草稿：自定义确认对话框 → POST /admin/posts → 302 /admin/posts/{id}/edit
  const createRes = page.waitForResponse(
    (r) => r.request().method() === "POST" && /\/admin\/posts$/.test(r.request().url()),
  );
  await page.locator('button[data-action="draft"]').click();
  await acceptConfirmDialog(page);
  await createRes;
  await page.waitForURL("**/admin/posts/*/edit");

  // 编辑页（Vditor 已回填草稿正文）→ 发布；slug 从 window._post 读取
  await page.locator("#editor .ProseMirror").waitFor({ state: "visible" });
  const slug = await page.evaluate(() => (window as any)._post.slug);
  const updateRes = page.waitForResponse(
    (r) => r.request().method() === "POST" && /\/admin\/posts\/\d+\/update$/.test(r.request().url()),
  );
  await page.locator('button[data-action="published"]').click();
  await acceptConfirmDialog(page);
  await updateRes;

  // 前台文章页：标题、正文、阅读量
  await page.goto(`/post/${slug}`);
  await expect(page.locator("article.post h1")).toHaveText(title);
  await expect(page.locator(".md-body")).toContainText(body);
  await expect(page.locator("article.post .meta")).toContainText("阅读");
});
