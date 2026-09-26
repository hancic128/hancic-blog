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

test("帮助页采用单列居中布局：目录在正文上方、页面无网格错位", async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto("/admin/help");

  // 1) 目录可见
  const toc = page.locator(".api-toc");
  await expect(toc).toBeVisible();

  // 2) .panel 是普通单列文档流：曾用 grid 把目录与正文排成两列，每个
  //    .panel-section 独占一行，行高互相撑开 → 目录列下方大片空白、正文被拉开
  const panel = page.locator(".panel").first();
  expect(await panel.evaluate((el) => getComputedStyle(el).display)).toBe("block");
  expect(await toc.evaluate((el) => getComputedStyle(el).position)).toBe("static");

  // 3) 目录在正文上方（目录底边 ≤ 第一个分区顶边）
  const tocBox = await toc.boundingBox();
  const sectionBox = await page.locator(".panel-section").first().boundingBox();
  expect(tocBox).not.toBeNull();
  expect(sectionBox).not.toBeNull();
  if (tocBox && sectionBox) {
    expect(tocBox.y + tocBox.height).toBeLessThanOrEqual(sectionBox.y + 1);
  }

  // 4) 正文容器水平居中（margin-left ≈ margin-right），与其它后台页面一致
  const contentBox = await page.locator(".admin-content").boundingBox();
  const shellBox = await page.locator(".admin-main").boundingBox();
  expect(contentBox).not.toBeNull();
  expect(shellBox).not.toBeNull();
  if (contentBox && shellBox) {
    const leftGap = contentBox.x - shellBox.x;
    const rightGap = shellBox.x + shellBox.width - (contentBox.x + contentBox.width);
    // 帮助页正文自身有 max-width 限宽；两边的留白差 < 4px 表示水平居中
    expect(Math.abs(leftGap - rightGap)).toBeLessThan(4);
  }
});

test("品牌区点击新标签页打开博客首页；侧栏底部显示版本徽章与最近部署时间", async ({ page, context }) => {
  await loginAsAdmin(page);
  await page.goto("/admin");

  // 侧栏底部 admin-meta：版本徽章 + 部署时间
  const meta = page.locator(".admin-side .admin-meta");
  await expect(meta).toBeVisible();

  const versionTag = meta.locator(".admin-version-tag");
  await expect(versionTag).toBeVisible();
  // 版本徽章形如 vX.Y.Z（CARGO_PKG_VERSION 前缀 v）
  await expect(versionTag).toHaveText(/^v\d+\.\d+\.\d+$/);

  const deploy = meta.locator(".admin-deploy");
  await expect(deploy).toBeVisible();
  // 部署时间形如「最近部署 + YYYY-MM-DD HH:MM:SS 部署」（含秒）
  await expect(deploy).toHaveText(/^最近部署 \d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2} 部署$/);
  // 标签与时间拆成两行后必须完整可见（曾单行 206px > 191px 可用宽度被 ellipsis 截断）
  const clipped = await deploy.evaluate((el) => el.scrollWidth > el.clientWidth + 1);
  expect(clipped).toBe(false);

  // 品牌区 hover 不出现下划线（覆盖全局 a:hover { text-decoration: underline }）
  const brand = page.locator("a.admin-brand");
  await expect(brand).toHaveAttribute("href", "/");
  await expect(brand).toHaveAttribute("target", "_blank");
  await brand.hover();
  const td = await brand.evaluate((el) => getComputedStyle(el).textDecorationLine);
  expect(td).toBe("none");

  // 品牌区为新标签页链接：点击后在新增的 page 上落到博客首页
  const [opened] = await Promise.all([context.waitForEvent("page"), brand.click()]);
  await opened.waitForLoadState("domcontentloaded");
  expect(new URL(opened.url()).pathname).toBe("/");
});

test("整页跳转加载动画：点击导航立刻出 spinner + 遮罩，新页续接后自动收起", async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto("/admin");

  const bar = page.locator(".admin-loading-bar");
  await expect(bar).toHaveCount(1);
  // 空闲态：进度条 ::before 透明（无 is-loading）
  expect(await bar.evaluate((el) => getComputedStyle(el, "::before").opacity)).toBe("0");
  // keyframes 定义存在，确认动画生效（顶栏进度条 + 居中 spinner）
  expect(await bar.evaluate((el) => getComputedStyle(el, "::before").animationName)).toBe(
    "admin-loading-slide",
  );
  expect(await bar.evaluate((el) => getComputedStyle(el, "::after").animationName)).toBe(
    "admin-loading-spin",
  );

  // ① 点导航：立刻进入加载态。旧版只有一条 3px 顶栏细线，用户基本看不到；
  //    现在是顶栏进度条 + 居中 spinner + 半透明遮罩。
  //    合成 click 前先挂一个捕获阶段 preventDefault，只跑事件处理、不真的跳转，
  //    否则断言会被页面卸载打断。
  await page.evaluate(() => {
    const link = Array.from(document.querySelectorAll("a.admin-nav-item")).find((el) =>
      (el.textContent || "").includes("帮助"),
    );
    if (!link) throw new Error("侧栏未找到「帮助」菜单项");
    link.addEventListener("click", (e) => e.preventDefault(), { capture: true });
    link.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
  });
  await expect(bar).toHaveClass(/is-loading/);
  // 两个可见元素都有 0.15s 过渡，等它们过渡到位再断言（否则读到的是中间值）
  await expect
    .poll(
      () =>
        bar.evaluate((el) => ({
          spinner: getComputedStyle(el, "::after").opacity,
          overlay: getComputedStyle(el).backgroundColor,
        })),
      { timeout: 2000 },
    )
    .toEqual({ spinner: "1", overlay: "rgba(0, 0, 0, 0.32)" });
  // 跨页续接标记已写入（新文档靠它补足最短可见时长）
  const marker = await page.evaluate(() => sessionStorage.getItem("admin-nav-loading-at"));
  expect(Number(marker)).toBeGreaterThan(0);

  // ② 整页跳转：新文档解析时立刻「续接」加载态（内网 SSR 再快也看得见），
  //    补足最短显示时长（400ms）后自动收起，不留后遗症。
  //    这里重新盖一次标记，等价于「上一页刚刚点了导航」。
  await page.evaluate(() => sessionStorage.setItem("admin-nav-loading-at", String(Date.now())));
  await page.goto("/admin/help", { waitUntil: "commit" });
  await expect
    .poll(
      () => bar.evaluate((el) => el.classList.contains("is-loading")).catch(() => false),
      { timeout: 3000 },
    )
    .toBe(true);
  await expect(bar).not.toHaveClass(/is-loading/, { timeout: 3000 });
  expect(await page.evaluate(() => sessionStorage.getItem("admin-nav-loading-at"))).toBeNull();
});

test("后台内容区在宽屏下自适应变宽（父容器限宽时取 min，不超过 1600）", async ({ page }) => {
  await loginAsAdmin(page);

  // 默认 desktop 视口 1280：内容区宽度受侧栏 236px + 内容 padding 限制
  // （关键修复前是固定 max-width: 1120px，1280 视口下被父容器卡到 ≤1044；
  // 现在改为 clamp，父容器宽度足够时能真正撑到 1120+）
  await page.goto("/admin");
  const w1280 = await page.locator(".admin-content").evaluate(
    (el) => el.getBoundingClientRect().width,
  );
  // 不超过视口（去滚动条）和父容器（侧栏 236 + padding）
  expect(w1280).toBeLessThanOrEqual(1280 - 236);

  // 拉到 1920：内容区应随之拉宽（70vw = 1344，clamp 上限 1600）
  await page.setViewportSize({ width: 1920, height: 1080 });
  await page.reload();
  const w1920 = await page.locator(".admin-content").evaluate(
    (el) => el.getBoundingClientRect().width,
  );
  // 关键断言：宽屏比窄屏更宽
  expect(w1920).toBeGreaterThan(w1280 + 100);
  // clamp 上限 1600，且有左右 padding——实测应 < 1600
  expect(w1920).toBeLessThanOrEqual(1600);
});
