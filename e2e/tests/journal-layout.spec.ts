import { test, expect } from "@playwright/test";
import { loginAsAdmin, acceptConfirmDialog } from "../helpers";

/**
 * journal 主题双栏版式：
 *   1. layout-main + layout-side 占满 layout-two-col（不再固定 820px 居中留白）
 *   2. 右侧悬浮侧栏用左侧竖向边线（与左侧常驻侧栏同款）
 * 走 ?theme_preview=journal 只读预览，不改动 data-e2e 的激活主题。
 */
test("journal 双栏占满容器，右侧栏用左侧竖向边线", async ({ page }) => {
  const title = `journal 版式 ${Date.now()}`;
  await loginAsAdmin(page);

  // 建一篇带小标题的文章（右侧目录才有内容，双栏才成立）
  await page.goto("/admin/posts/new");
  await page.locator("#post-title").fill(title);
  await page.locator("#editor .ProseMirror").waitFor({ state: "visible" });
  await page.locator("#editor .ProseMirror").click();
  await page.keyboard.type("## 第一节\n\n正文段落一。\n\n## 第二节\n\n正文段落二。");

  // 先存草稿（留在编辑页），取 slug 后再发布
  const createRes = page.waitForResponse(
    (r) => r.request().method() === "POST" && /\/admin\/posts$/.test(r.request().url()),
  );
  await page.locator('button[data-action="draft"]').click();
  await acceptConfirmDialog(page);
  await createRes;
  await page.waitForURL("**/admin/posts/*/edit");
  const slug = await page.evaluate(() => (window as any)._post.slug);

  const publishRes = page.waitForResponse(
    (r) => r.request().method() === "POST" && /\/admin\/posts\/\d+\/update$/.test(r.request().url()),
  );
  await page.locator('button[data-action="published"]').click();
  await acceptConfirmDialog(page);
  await publishRes;

  // /post/{slug} 会 302 到规范 URL 并丢掉查询串：先落规范 URL，再带预览参数访问
  await page.goto(`/post/${slug}`);
  const postUrl = new URL(page.url());
  await page.goto(`${postUrl.pathname}?theme_preview=journal`);
  await expect(page.locator('link[rel="stylesheet"]').first()).toHaveAttribute(
    "href",
    /\/theme\/journal\/static\/style\.css$/,
  );

  const box = (sel: string) =>
    page.locator(sel).evaluate((el) => {
      const r = el.getBoundingClientRect();
      const cs = getComputedStyle(el);
      return {
        x: r.x,
        width: r.width,
        right: r.right,
        borderLeft: cs.borderLeftWidth,
        display: cs.display,
      };
    });

  const twoCol = await box(".layout-two-col");
  const main = await box(".layout-main");
  const side = await box(".layout-side");

  expect(side.display).not.toBe("none");
  expect(side.borderLeft).toBe("1px");
  // 两列 + 列间距正好占满 layout-two-col，且左右都贴到容器边缘
  const gap = twoCol.width - main.width - side.width;
  expect(gap).toBeGreaterThan(0);
  expect(gap).toBeLessThan(48);
  expect(main.x - twoCol.x).toBeLessThan(2);
  expect(twoCol.right - side.right).toBeLessThan(2);

  // 阅读模式：仍回到 46rem 限宽（736px）并在视口中居中
  // （双栏规则不得覆盖 .reading-mode .layout-main，否则会撑成满宽）
  await page.locator(".reading-float").click();
  const reading = await box(".layout-main");
  expect(reading.width).toBeLessThan(800);
  const viewport = page.viewportSize()!;
  const leftGap = reading.x;
  const rightGap = viewport.width - (reading.x + reading.width);
  expect(Math.abs(leftGap - rightGap)).toBeLessThan(4);
  await page.locator(".reading-float").click();
});
