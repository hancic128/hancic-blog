import { test, expect } from "@playwright/test";
import { readFileSync } from "node:fs";
import path from "node:path";
import { loginAsAdmin, pastePngToEditor, acceptConfirmDialog, clearEditor } from "../helpers";

const FIXTURE = path.resolve(__dirname, "../fixtures/1x1.png");
const PNG_B64 = readFileSync(FIXTURE).toString("base64");

// 同文件内用例串行执行（mobile project 独立于 desktop 的发布用例）
test.describe.configure({ mode: "serial" });

const TITLE = `移动端文章 ${Date.now()}`;
const BODY = "移动端正文内容 e2e-mobile-body";
const MOMENT_TEXT = `移动端说说 ${Date.now()} e2e-mobile-moment 这是一条足够长的移动端说说正文，确保超过预览截断阈值以启用折叠展开功能`;

/**
 * 375px 移动端视口用例：
 * 1. 首页导航汉堡可展开/收起
 * 2. /moments 发一条带图说说
 * 3. 新建并发布带图文章
 * 4. 文章页无横向溢出（scrollWidth <= clientWidth）
 */
test("首页移动端导航可展开/收起", async ({ page }) => {
  await page.goto("/");
  const nav = page.locator("#site-nav");
  await expect(nav).toBeHidden();
  await page.locator("#nav-toggle").click();
  await expect(nav).toBeVisible();
  await expect(nav).toHaveClass(/open/);
  await page.locator("#nav-toggle").click();
  await expect(nav).toBeHidden();
});

test("375px 发布带图说说并前台可见", async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto("/admin/moments");
  await page.locator("#moment-content").fill(MOMENT_TEXT);
  await page.locator("#moment-file").setInputFiles(FIXTURE);
  await expect(page.locator(".moment-pick img")).toBeVisible({ timeout: 15_000 });

  const createRes = page.waitForResponse(
    (r) => r.request().method() === "POST" && /\/admin\/moments$/.test(r.request().url()),
  );
  await page.locator('#moment-form button[type="submit"]').click();
  await createRes;
  await page.waitForURL("**/admin/moments");

  await page.goto("/moments");
  // 时间线默认折叠：先展开该条
  const body = page
    .locator(".moment-body")
    .filter({ has: page.locator(`text=${MOMENT_TEXT}`) })
    .first();
  await body.locator(".moment-toggle").click();
  const card = page.locator(".moment-item", { hasText: MOMENT_TEXT });
  await expect(card).toBeVisible();
  await expect(card.locator(".moment-grid img").first()).toBeVisible({
    timeout: 15_000,
  });
});

test("375px 发布带图文章", async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto("/admin/posts/new");
  await page.locator("#post-title").fill(TITLE);
  await page.locator("#editor .ProseMirror").waitFor({ state: "visible" });
  await page.locator("#editor .ProseMirror").click();
  // 清空新建页预填的 Markdown 语法模板
  await clearEditor(page);
  await page.keyboard.type(BODY);

  // 粘贴图片进正文（复用与 paste-upload.spec 相同的触发方式）
  const uploadRes = page.waitForResponse(
    (r) => r.request().method() === "POST" && r.url().includes("/api/uploads"),
    { timeout: 15_000 },
  );
  await pastePngToEditor(page, PNG_B64);
  await uploadRes;
  await expect(page.locator('#editor img[src^="/uploads/"]').first()).toBeVisible({
    timeout: 15_000,
  });

  // 直接发布（新建即发布）：发布后自动返回列表
  const createRes = page.waitForResponse(
    (r) => r.request().method() === "POST" && /\/admin\/posts$/.test(r.request().url()),
  );
  await page.locator('button[data-action="published"]').click();
  await acceptConfirmDialog(page);
  await createRes;
  await page.waitForURL("**/admin/posts");

  // 固定链接由系统生成：从列表进入编辑页读 slug 后访问前台
  await page.locator(`a[href*="/edit"]`, { hasText: TITLE }).first().click();
  await page.locator("#editor .ProseMirror").waitFor({ state: "visible" });
  const slug = await page.evaluate(() => (window as any)._post.slug);
  await page.goto(`/post/${slug}`);
  await expect(page.locator("article.post h1")).toHaveText(TITLE);
  await expect(page.locator(".md-body")).toContainText(BODY);
  await expect(page.locator('.md-body img[src^="/uploads/"]').first()).toBeVisible({
    timeout: 15_000,
  });
});

test("文章页无横向溢出（375px）", async ({ page }) => {
  // 打开归档页第一篇已发布文章（固定链接为系统 uuid，无法预知）
  await page.goto("/archives");
  const firstPost = page.locator(".post-list-item a[href^='/post/']").first();
  await firstPost.click();
  await expect(page.locator("article.post h1")).toBeVisible();
  const { scrollWidth, clientWidth } = await page.evaluate(() => {
    const de = document.documentElement;
    return { scrollWidth: de.scrollWidth, clientWidth: de.clientWidth };
  });
  expect(scrollWidth).toBeLessThanOrEqual(clientWidth);
});
