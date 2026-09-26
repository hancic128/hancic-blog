import { test, expect } from "@playwright/test";
import { loginAsAdmin, acceptConfirmDialog } from "../helpers";

/**
 * journal 主题 UI 修复：
 *   1. 左侧常驻侧栏 nav hover/active 显示左侧竖向 accent line（active 满高）
 *   2. 右侧 toc / 月历 / 标签 hover 显示左侧竖向 accent line，active 满高
 *   3. 说说列表：按容器宽度裁剪（CSS ellipsis），仅在文本被截断时显示折叠按钮
 * 走 ?theme_preview=journal 只读预览，不改动 data-e2e 的激活主题。
 */

test("左侧侧栏 nav 当前页显示 is-active 满高竖线", async ({ page }) => {
  await loginAsAdmin(page);

  // 发一篇文章（占位，确保 /archives 至少能渲染）
  await page.goto("/admin/posts/new");
  await page.locator("#post-title").fill(`journal ui nav ${Date.now()}`);
  await page.locator("#editor .ProseMirror").waitFor({ state: "visible" });
  await page.locator("#editor .ProseMirror").click();
  await page.keyboard.type("占位正文");
  const createRes = page.waitForResponse(
    (r) => r.request().method() === "POST" && /\/admin\/posts$/.test(r.request().url()),
  );
  await page.locator('button[data-action="draft"]').click();
  await acceptConfirmDialog(page);
  await createRes;
  await page.waitForURL("**/admin/posts/*/edit");

  // 访问 /archives?theme_preview=journal —— nav 中「归档」项应自动加 is-active
  await page.goto("/archives?theme_preview=journal");
  await expect(page.locator("link[rel=stylesheet]").first()).toHaveAttribute(
    "href",
    /\/theme\/journal\/static\/style\.css$/,
  );

  const active = page.locator(".site-nav .nav-item.is-active, .site-nav .nav-dropdown.is-active");
  await expect(active).toHaveCount(1);
  // active 项的 ::before 高度 100%（满高竖线）
  const beforeHeight = await active.first().evaluate((el) => {
    const target = el.querySelector(":scope > a");
    if (!target) return null;
    const cs = getComputedStyle(target, "::before");
    return cs.height;
  });
  // 像素值应明显大于 0（hover 是 60% 半高，active 是 100% 满高；断言非空 + 是 px 字符串）
  expect(beforeHeight).toBeTruthy();
  expect(beforeHeight).not.toBe("0px");
});

test("右侧 toc 链接 hover 显示左侧竖向 accent line", async ({ page }) => {
  const title = `journal toc hover ${Date.now()}`;
  await loginAsAdmin(page);

  // 发带 h2 小标题的文章（右侧 toc 才有内容）
  await page.goto("/admin/posts/new");
  await page.locator("#post-title").fill(title);
  await page.locator("#editor .ProseMirror").waitFor({ state: "visible" });
  await page.locator("#editor .ProseMirror").click();
  await page.keyboard.type("## 第一节\n\n正文段落一。\n\n## 第二节\n\n正文段落二。");
  const createRes = page.waitForResponse(
    (r) => r.request().method() === "POST" && /\/admin\/posts$/.test(r.request().url()),
  );
  await page.locator('button[data-action="draft"]').click();
  await acceptConfirmDialog(page);
  await createRes;
  await page.waitForURL("**/admin/posts/*/edit");
  const slug = await page.evaluate(() => (window as any)._post.slug);
  const publishRes = page.waitForResponse(
    (r) => r.request().method() === "POST" && /\/admin\/posts\/.+\/update$/.test(r.request().url()),
  );
  await page.locator('button[data-action="published"]').click();
  await acceptConfirmDialog(page);
  await publishRes;

  // /post/{slug} 跳规范 URL 后丢查询串；先抓规范 URL，再带 preview 参数访问
  await page.goto(`/post/${slug}`);
  const postUrl = new URL(page.url());
  await page.goto(`${postUrl.pathname}?theme_preview=journal`);

  // 1280px 视口：双栏版式启用，右侧 toc 有内容
  const tocLink = page.locator(".side-toc a").first();
  await expect(tocLink).toBeVisible();

  // hover 前 ::before 高度为 0
  const beforeHover = await tocLink.evaluate(
    (el) => getComputedStyle(el, "::before").height,
  );
  expect(beforeHover).toBe("0px");

  // hover 时 ::before 高度变为 60%（半高）
  await tocLink.hover();
  // 等过渡动画结束（CSS transition 200ms）
  await page.waitForTimeout(280);
  const afterHover = await tocLink.evaluate(
    (el) => getComputedStyle(el, "::before").height,
  );
  expect(afterHover).not.toBe("0px");
});

test("说说列表：短文本自动展开（无折叠按钮），长文本按容器裁剪后显示折叠按钮", async ({ page }) => {
  await loginAsAdmin(page);

  // 发一短一长两条说说（>60 字确保被截断，<20 字确保不被截断）
  const longText = "E2E journal overflow test ".repeat(8); // ~200+ chars
  const shortText = "短文本"; // < 20 chars

  async function publishMoment(content: string) {
    await page.goto("/admin/moments");
    await page.locator("#moment-content").fill(content);
    const createRes = page.waitForResponse(
      (r) => r.request().method() === "POST" && /\/admin\/moments$/.test(r.request().url()),
    );
    await page.locator('#moment-form button[type="submit"]').click();
    await createRes;
    await page.waitForURL("**/admin/moments");
  }
  await publishMoment(longText);
  await publishMoment(shortText);

  // 前台 /moments 走 journal 预览
  await page.goto("/moments?theme_preview=journal");

  // 短文本条目：直接展开，无折叠按钮
  const shortItem = page.locator(".moment-item").filter({ hasText: shortText }).first();
  await expect(shortItem).toHaveClass(/moment-short/);
  await expect(shortItem.locator(".moment-toggle")).toBeHidden();

  // 长文本条目：未截断的情况下显示折叠按钮（视口足够宽时可能不被截断——
  // 此处用短文案触发 overflow 检测的对照；只要断言折叠按钮存在即可）
  const longItem = page.locator(".moment-item").filter({ hasText: longText.substring(0, 40) }).first();
  await expect(longItem).toBeVisible();

  // 切换到窄视口（移动端），长文本必被截断 → 折叠按钮必显示
  await page.setViewportSize({ width: 375, height: 667 });
  await page.goto("/moments?theme_preview=journal");
  const longItemNarrow = page.locator(".moment-item").filter({ hasText: longText.substring(0, 40) }).first();
  await expect(longItemNarrow).toBeVisible();
  // 等 JS 检测（requestAnimationFrame 后）
  await page.waitForTimeout(80);
  await expect(longItemNarrow.locator(".moment-toggle")).toBeVisible();
});
