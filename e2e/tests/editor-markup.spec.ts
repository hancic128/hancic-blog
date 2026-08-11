import { test, expect } from "@playwright/test";
import { loginAsAdmin, clearEditor } from "../helpers";

/**
 * 新建文章编辑器的标记与格式行为（milkdown WYSIWYG）：
 * - 工具栏含「正文」按钮，可将标题转回段落
 * - 引用按钮在引用内点击 = 解除引用（不嵌套）；引用块首 Backspace 解除引用
 * - 列表光标行只显示隐藏标记，不同时出现原生序号；块首 Backspace 脱离列表
 * - 标题/代码块块首 Backspace 删除标记（降级 / 转段落）
 * - GFM 表格渲染；mermaid 代码块实时预览
 * - 标签 chips 显示在输入框下方
 */
test("编辑器标记行为：正文/引用不嵌套/块首退格删标记/表格/mermaid/标签chips", async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto("/admin/posts/new");
  await page.locator("#editor .ProseMirror").waitFor({ state: "visible" });
  await page.locator("#editor .ProseMirror").click();
  await clearEditor(page);

  const pm = page.locator("#editor .ProseMirror");

  // ---- 1. 正文按钮：输入文字 → 标题 → 点正文按钮转回段落 ----
  await page.keyboard.type("正文测试文字");
  await page.locator("#md-h1").click();
  await expect(pm.locator("h1").first()).toContainText("正文测试文字");
  await page.locator("#md-p").click();
  await expect(pm.locator("h1")).toHaveCount(0);

  // ---- 2. 引用按钮：第一点包裹引用，第二点在引用内点击 = 解除引用 ----
  await page.locator("#md-quote").click();
  await expect(pm.locator("blockquote")).toHaveCount(1);
  await page.locator("#md-quote").click();
  await expect(pm.locator("blockquote")).toHaveCount(0);

  // ---- 3. 引用块首 Backspace：解除引用 ----
  await page.keyboard.press("Enter");
  await page.keyboard.type("引用内容");
  await page.locator("#md-quote").click();
  await expect(pm.locator("blockquote")).toHaveCount(1);
  // 点击隐藏标记「> 」把光标定位到块首
  await pm.locator(".hancic-md-symbol").first().click();
  await page.keyboard.press("Backspace");
  await expect(pm.locator("blockquote")).toHaveCount(0);

  // ---- 4. 有序列表：光标行隐藏标记「1.」+ 原生序号关闭 ----
  await page.keyboard.type("列表第一项");
  await page.locator("#md-ol").click();
  await expect(pm.locator(".hancic-md-symbol", { hasText: "1." })).toHaveCount(1);
  const liStyle = await pm.locator("li.hancic-md-no-marker").first().evaluate((el) =>
    getComputedStyle(el).listStyleType,
  );
  expect(liStyle).toBe("none");

  // ---- 5. 列表项块首 Backspace：脱离列表变段落 ----
  await pm.locator(".hancic-md-symbol").first().click();
  await page.keyboard.press("Backspace");
  await expect(pm.locator("ol")).toHaveCount(0);

  // ---- 6. 标题块首 Backspace：降级（H2 → H1） ----
  await page.keyboard.type("再次标题");
  await page.locator("#md-h2").click();
  await expect(pm.locator("h2")).toHaveCount(1);
  await pm.locator(".hancic-md-symbol").first().click();
  await page.keyboard.press("Backspace");
  await expect(pm.locator("h2")).toHaveCount(0);

  // ---- 7. 代码块块首 Backspace：转回段落 ----
  await page.keyboard.type("code 内容");
  await page.locator("#md-codeblock").click();
  // 代码块按钮弹出语言对话框：留空直接确定
  await page.locator(".modal-input").fill("");
  await page.locator(".modal-actions .btn.btn-primary").click();
  await expect(pm.locator("pre")).toHaveCount(1);
  // 代码块无隐藏标记 widget，点击 code 内容左上角把光标放到内容开头
  await pm.locator("pre code").first().click({ position: { x: 2, y: 8 } });
  await page.keyboard.press("Backspace");
  await expect(pm.locator("pre")).toHaveCount(0);

  // ---- 8. GFM 表格：setContent 表格 markdown → 编辑器出现 table ----
  await clearEditor(page);
  await page.evaluate(async () => {
    const ed = (window as any)._hancicEditor;
    await ed.setContent("| a | b |\n|---|---|\n| 1 | 2 |");
  });
  await expect(pm.locator("table")).toHaveCount(1);
  await expect(pm.locator("table th").first()).toHaveText("a");

  // ---- 9. mermaid 代码块实时预览 ----
  await page.evaluate(async () => {
    const ed = (window as any)._hancicEditor;
    await ed.setContent("```mermaid\ngraph TD\n  A-->B\n```");
  });
  const preview = pm.locator(".hancic-mermaid-preview");
  await expect(preview).toBeVisible({ timeout: 8000 });
  await expect(preview.locator("svg").first()).toBeVisible({ timeout: 8000 });

  // ---- 10. 标签 chips 在输入框下方 ----
  const tagInput = page.locator("#post-tags");
  const chips = page.locator("#tag-chips");
  await tagInput.fill("e2etag");
  await tagInput.press("Enter");
  await expect(chips).toContainText("e2etag");
  const chipBox = await chips.boundingBox();
  const inputBox = await tagInput.boundingBox();
  expect(chipBox && inputBox ? chipBox.y >= inputBox.y + inputBox.height - 1 : false).toBe(true);
});
