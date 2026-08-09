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
 * 向 Vditor IR 编辑器派发「粘贴图片」事件：构造 ClipboardEvent，
 * clipboardData 由 DataTransfer 提供（含一个 PNG File），dispatch 到
 * IR 模式的编辑面 `.vditor-reset`（contenteditable 元素，Vditor 的 paste
 * 监听器绑在其上），真实触发 Vditor 的 upload.handler → POST /api/uploads。
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
    const target =
      document.querySelector(".vditor-reset") ||
      document.querySelector(".vditor-ir");
    target?.dispatchEvent(evt);
  }, pngBase64);
}
