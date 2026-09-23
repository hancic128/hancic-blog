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
