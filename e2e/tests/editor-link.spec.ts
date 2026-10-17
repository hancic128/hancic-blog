import { test, expect } from "@playwright/test";
import { clearEditor, loginAsAdmin } from "../helpers";

async function confirmPrompt(page: import("@playwright/test").Page, value: string) {
  const input = page.locator(".modal-input");
  await expect(input).toBeVisible();
  await input.fill(value);
  await page.locator(".modal-actions .btn-primary").click();
}

test("链接按钮给选中文字套链接", async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto("/admin/posts/new");
  const editor = page.locator("#editor .ProseMirror");
  await editor.waitFor({ state: "visible" });
  await editor.click();
  await clearEditor(page);
  await page.keyboard.type("链接文字");
  await page.keyboard.press("ControlOrMeta+A");

  await page.locator("#md-insert-link").click();
  await confirmPrompt(page, "https://example.com");

  const link = page.locator('#editor a[href="https://example.com"]');
  await expect(link).toHaveText("链接文字");
});

test("无选区时链接按钮追问显示文字", async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto("/admin/posts/new");
  const editor = page.locator("#editor .ProseMirror");
  await editor.waitFor({ state: "visible" });
  await editor.click();
  await clearEditor(page);

  await page.locator("#md-insert-link").click();
  await confirmPrompt(page, "https://example.com");
  await confirmPrompt(page, "显示文字");

  const link = page.locator('#editor a[href="https://example.com"]');
  await expect(link).toContainText("显示文字");
});

test("光标在链接内：按钮预填当前地址并改地址", async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto("/admin/posts/new");
  const editor = page.locator("#editor .ProseMirror");
  await editor.waitFor({ state: "visible" });
  await editor.click();
  await clearEditor(page);

  // 用输入规则造一个链接，光标落在链接末尾
  await page.keyboard.type("[链接文字](https://old.example)");
  await expect(page.locator('#editor a[href="https://old.example"]')).toHaveCount(1);

  await page.locator("#md-insert-link").click();
  const input = page.locator(".modal-input");
  await expect(input).toHaveValue("https://old.example"); // 预填当前地址
  await input.fill("https://new.example");
  await page.locator(".modal-actions .btn-primary").click();

  await expect(page.locator('#editor a[href="https://new.example"]')).toContainText("链接文字");
  await expect(page.locator('#editor a[href="https://old.example"]')).toHaveCount(0);
});

test("光标在链接内：可取消链接（保留文字）", async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto("/admin/posts/new");
  const editor = page.locator("#editor .ProseMirror");
  await editor.waitFor({ state: "visible" });
  await editor.click();
  await clearEditor(page);

  await page.keyboard.type("前文[链接文字](https://example.com)后文");
  await expect(page.locator("#editor a")).toHaveCount(1);

  // 光标在文末，左移两次越过「后文」落到链接内
  await page.keyboard.press("ArrowLeft");
  await page.keyboard.press("ArrowLeft");
  await page.locator("#md-insert-link").click();
  await page.locator(".modal-actions .btn-danger").click(); // 取消链接

  await expect(page.locator("#editor a")).toHaveCount(0);
  await expect(page.locator("#editor .ProseMirror")).toContainText("前文链接文字后文");
});

test("选中链接文字：按钮进入编辑（预填地址），可改地址", async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto("/admin/posts/new");
  const editor = page.locator("#editor .ProseMirror");
  await editor.waitFor({ state: "visible" });
  await editor.click();
  await clearEditor(page);

  await page.keyboard.type("[可编辑](https://a.example)");
  // 选回链接文字：光标在末尾，Shift+左选一个字符，落在链接内
  await page.keyboard.press("Shift+ArrowLeft");
  await page.locator("#md-insert-link").click();
  const input = page.locator(".modal-input");
  await expect(input).toHaveValue("https://a.example");
  await input.fill("https://b.example");
  await page.locator(".modal-actions .btn-primary").click();
  await expect(page.locator('#editor a[href="https://b.example"]')).toHaveCount(1);
});
