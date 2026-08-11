import type { Page } from "@playwright/test";

/** global-setup 设置的测试密码 */
export const ADMIN_PASSWORD = "e2e-password-123";

/** 后台登录（走真实 UI 表单，含 CSRF） */
export async function loginAsAdmin(page: Page): Promise<void> {
  await page.goto("/admin/login");
  await page.locator('input[name="password"]').fill(ADMIN_PASSWORD);
  await page.locator('button[type="submit"]').click();
  await page.waitForURL("**/admin");
}

/**
 * 清空 milkdown 编辑器内容（E2E 用：经 bundle 暴露的 _hancicEditor 钩子可靠清空，
 * 避免 Ctrl+A/Delete 在 ProseMirror 中删不干净）。
 */
export async function clearEditor(page: Page): Promise<void> {
  await page.evaluate(async () => {
    const ed = (window as any)._hancicEditor;
    if (ed && ed.setContent) await ed.setContent("");
  });
}

/**
 * 点后台自定义确认对话框的「确认」按钮。
 * 发布/存草稿/删除等带 data-confirm 的操作改用自定义对话框
 * （不再触发原生 confirm），测试需显式点确认。
 */
export async function acceptConfirmDialog(page: Page): Promise<void> {
  const overlay = page.locator(".modal-overlay");
  await overlay.waitFor({ state: "visible" });
  await overlay.locator(".modal-actions .btn:last-child").click();
}

/**
 * 向 milkdown 编辑器派发「粘贴图片」事件：构造 ClipboardEvent，
 * clipboardData 由 DataTransfer 提供（含一个 PNG File），dispatch 到
 * 编辑器根容器 `#editor`（milkdown bundle 在容器上以捕获阶段监听 paste），
 * 真实触发上传处理 → POST /api/uploads。
 */
export async function pastePngToEditor(page: Page, pngBase64: string): Promise<void> {
  await page.evaluate((b64) => {
    const bin = atob(b64);
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    const file = new File([bytes], "e2e-upload.png", { type: "image/png" });
    const dt = new DataTransfer();
    dt.items.add(file);
    const evt = new ClipboardEvent("paste", {
      clipboardData: dt,
      bubbles: true,
      cancelable: true,
    });
    document.getElementById("editor")?.dispatchEvent(evt);
  }, pngBase64);
}
