import { test, expect } from "@playwright/test";
import { loginAsAdmin, pastePngToEditor } from "../helpers";

const PNG_B64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";

test("上传接口返回 HTML 时显示可读错误而非 JSON 解析异常", async ({ page }) => {
  await loginAsAdmin(page);
  await page.route("**/api/uploads", (route) =>
    route.fulfill({
      status: 502,
      contentType: "text/html",
      body: "<html><body>Bad Gateway</body></html>",
    }),
  );
  await page.goto("/admin/posts/new");
  await page.locator("#editor .ProseMirror").waitFor({ state: "visible" });
  await page.locator("#editor .ProseMirror").click();

  await pastePngToEditor(page, PNG_B64);
  const toast = page.locator(".toast-error");
  await expect(toast).toBeVisible();
  await expect(toast).toContainText("上传失败");
  await expect(toast).not.toContainText("Unexpected token");
});
