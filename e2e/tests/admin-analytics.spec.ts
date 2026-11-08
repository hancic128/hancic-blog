import { test, expect } from "@playwright/test";
import { loginAsAdmin } from "../helpers";

/**
 * 访问分析页（/admin/analytics）：
 * 侧栏入口存在、8 个板块齐全、热力图铺出 7×24 格、图表数据已注入。
 */
test("访问分析页：板块齐全、热力图铺满 7×24 格", async ({ page }) => {
  await loginAsAdmin(page);

  // 侧栏导航项存在且可点进
  const navItem = page.locator('.admin-nav-item[href$="/admin/analytics"]');
  await expect(navItem).toHaveCount(1);
  await expect(navItem).toContainText("访问分析");

  await page.goto("/admin/analytics");
  await expect(page).toHaveTitle(/访问分析/);

  // 主要板块标题
  for (const heading of [
    "设备构成",
    "浏览器与操作系统",
    "来源明细",
    "搜索关键词",
    "时段热力",
    "单篇来源下钻",
    "实时访问流",
  ]) {
    await expect(page.getByRole("heading", { name: new RegExp(heading) })).toHaveCount(1);
  }

  // 统计卡：独立访客 / 真实阅读 / 新访客 / 回访访客 / 爬虫 / 人均阅读
  await expect(page.locator(".analytics-stat-grid .stat-card")).toHaveCount(6);
  for (const label of ["独立访客", "新访客", "回访访客"]) {
    await expect(page.locator(".analytics-stat-grid")).toContainText(label);
  }

  // 卡片排成两行三列（基础 .stat-grid 规则的 span 3 会盖过本页的 span 2，
  // 曾排成 3+2+1 的锯齿形）——比较同一行内各卡片的纵坐标
  const tops = await page
    .locator(".analytics-stat-grid .stat-card")
    .evaluateAll((els) => els.map((el) => Math.round(el.getBoundingClientRect().top)));
  expect(new Set(tops).size).toBe(2); // 两行
  expect(tops.slice(0, 3).every((t) => t === tops[0])).toBe(true); // 前三张同行
  expect(tops.slice(3).every((t) => t === tops[3])).toBe(true); // 后三张同行

  // 热力图：7 行（周一至周日）× 24 列
  const heatmap = page.locator("#heatmap");
  await expect(heatmap).toHaveCount(1);
  await expect(heatmap.locator(".heatmap-row")).toHaveCount(7);
  await expect(heatmap.locator(".heatmap-cell")).toHaveCount(7 * 24);
  await expect(heatmap.locator(".heatmap-label")).toHaveText([
    "周一",
    "周二",
    "周三",
    "周四",
    "周五",
    "周六",
    "周日",
  ]);

  // 注入的图表数据可解析，且含热力图 168 格
  const chart = await page.evaluate(() => (window as any).chartData);
  expect(chart).toBeTruthy();
  expect(Array.isArray(chart.heatmap)).toBe(true);
  expect(chart.heatmap.length).toBe(7 * 24);
  expect(Array.isArray(chart.labels)).toBe(true);
});

/** 区间筛选：快捷按钮与自定义日期都能切，且带参数回显。 */
test("访问分析页：区间筛选切换并保留参数", async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto("/admin/analytics");

  // 「全部」快捷按钮
  const allRange = page.locator('a.quick-range[href="?range=all"]');
  await expect(allRange).toHaveCount(1);
  await allRange.click();
  await expect(page).toHaveURL(/range=all/);
  // 「全部」模式下 from/to 输入框留空
  await expect(page.locator('input[name="from"]')).toHaveValue("");
  await expect(page.locator('input[name="to"]')).toHaveValue("");

  // 最近 7 天
  await page.goto("/admin/analytics?days=7");
  await expect(page.locator('input[name="from"]')).not.toHaveValue("");
  const from = await page.locator('input[name="from"]').inputValue();
  const to = await page.locator('input[name="to"]').inputValue();
  expect(from).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  expect(to).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  // 7 天窗口：from 应当是 to 之前 6 天
  const diffDays =
    (new Date(`${to}T00:00:00Z`).getTime() - new Date(`${from}T00:00:00Z`).getTime()) /
    86400000;
  expect(diffDays).toBe(6);
});
