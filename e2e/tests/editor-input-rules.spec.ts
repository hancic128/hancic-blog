import { test, expect, type Page } from "@playwright/test";
import { loginAsAdmin, clearEditor } from "../helpers";

/**
 * 编辑器 Typora 式输入规则（milkdown WYSIWYG）：
 * - `**加粗**` 输入完即转 <strong>（含紧贴中文，内置规则要求非单词边界、对中文不匹配）
 * - `[文字](url)` 输入完即转链接（内置完全没有链接输入规则）
 * 与既有 #/引用/列表/单星号斜体/图片规则并存，互不冲突。
 *
 * 注意：mdHint 会以非编辑 widget span 显示标记（如 `**`、`[`），会污染 textContent；
 * 断言一律先剥掉 .hancic-md-symbol 只比较真实文本。
 */
async function openEditor(page: Page) {
  await loginAsAdmin(page);
  await page.goto("/admin/posts/new");
  const pm = page.locator("#editor .ProseMirror");
  await pm.waitFor({ state: "visible" });
  await pm.click();
  await clearEditor(page);
  return pm;
}

/** 选中元素内、去掉 md 标记 widget 后的真实文本 */
async function cleanText(locator: import("@playwright/test").Locator): Promise<string> {
  return locator.evaluate((el) => {
    const clone = el.cloneNode(true) as HTMLElement;
    clone.querySelectorAll(".hancic-md-symbol").forEach((n) => n.remove());
    return clone.textContent || "";
  });
}

test("输入规则：**加粗** 转 <strong>", async ({ page }) => {
  const pm = await openEditor(page);
  await page.keyboard.type("**加粗**");
  const strong = pm.locator("strong");
  await expect(strong).toHaveCount(1);
  expect(await cleanText(strong)).toBe("加粗");
});

test("输入规则：紧贴中文的 **加粗** 也转换", async ({ page }) => {
  const pm = await openEditor(page);
  await page.keyboard.type("中文**加粗**结尾");
  const strong = pm.locator("strong");
  await expect(strong).toHaveCount(1);
  expect(await cleanText(strong)).toBe("加粗");
  expect(await cleanText(pm)).toBe("中文加粗结尾");
});

test("输入规则：[文字](url) 转链接", async ({ page }) => {
  const pm = await openEditor(page);
  await page.keyboard.type("[链接文字](https://example.com)");
  const link = pm.locator('a[href="https://example.com"]');
  await expect(link).toHaveCount(1);
  expect(await cleanText(link)).toBe("链接文字");
});

test("输入规则：链接后继续输入不并入链接（storedMarks 已复位）", async ({ page }) => {
  const pm = await openEditor(page);
  await page.keyboard.type("[链接](https://x.example)后面文字");
  const link = pm.locator('a[href="https://x.example"]');
  await expect(link).toHaveCount(1);
  expect(await cleanText(link)).toBe("链接");
  expect(await cleanText(pm)).toBe("链接后面文字");
});

test("输入规则：加粗与链接混排互不干扰", async ({ page }) => {
  const pm = await openEditor(page);
  await page.keyboard.type("看[这里](https://a.example)有**重点**");
  const link = pm.locator('a[href="https://a.example"]');
  const strong = pm.locator("strong");
  await expect(link).toHaveCount(1);
  await expect(strong).toHaveCount(1);
  expect(await cleanText(link)).toBe("这里");
  expect(await cleanText(strong)).toBe("重点");
  expect(await cleanText(pm)).toBe("看这里有重点");
});

test("输入规则：单星号斜体仍正常（不被加粗规则吞掉）", async ({ page }) => {
  const pm = await openEditor(page);
  await page.keyboard.type("*斜体*");
  const em = pm.locator("em");
  await expect(em).toHaveCount(1);
  expect(await cleanText(em)).toBe("斜体");
  await expect(pm.locator("strong")).toHaveCount(0);
});
