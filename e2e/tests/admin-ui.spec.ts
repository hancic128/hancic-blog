import { test, expect } from "@playwright/test";
import { loginAsAdmin } from "../helpers";

test("后台侧栏折叠按钮位于侧栏内且可折叠", async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto("/admin");

  const button = page.locator("#admin-side #side-collapse");
  await expect(button).toHaveCount(1);
  await expect(page.locator("#fab-group #side-collapse")).toHaveCount(0);

  const shell = page.locator("#admin-shell");
  await expect(shell).not.toHaveClass(/side-collapsed/);
  await button.click();
  await expect(shell).toHaveClass(/side-collapsed/);
  await button.click();
  await expect(shell).not.toHaveClass(/side-collapsed/);
});

test("帮助页目录固定在右侧且不遮挡正文", async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto("/admin/help");

  const toc = page.locator(".api-toc");
  await expect(toc).toBeVisible();
  const position = await toc.evaluate((el) => getComputedStyle(el).position);
  expect(position).toBe("fixed");

  const tocBox = await toc.boundingBox();
  const sectionBox = await page.locator(".panel-section").first().boundingBox();
  expect(tocBox).not.toBeNull();
  expect(sectionBox).not.toBeNull();
  if (tocBox && sectionBox) {
    const overlaps =
      tocBox.x < sectionBox.x + sectionBox.width &&
      tocBox.x + tocBox.width > sectionBox.x &&
      tocBox.y < sectionBox.y + sectionBox.height &&
      tocBox.y + tocBox.height > sectionBox.y;
    expect(overlaps).toBe(false);
  }
});
