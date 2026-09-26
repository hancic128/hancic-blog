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

test("帮助页目录固定在右侧悬浮（journal 同款竖线），滚动高亮当前区块且不遮挡正文", async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto("/admin/help");

  const toc = page.locator(".api-toc");
  await expect(toc).toBeVisible();

  // 1) 固定悬浮（不进入正文流）
  expect(await toc.evaluate((el) => getComputedStyle(el).position)).toBe("fixed");

  // 2) .panel 是普通单列文档流（曾用 grid 双列，行高互相撑开导致错位）
  const panel = page.locator(".panel").first();
  expect(await panel.evaluate((el) => getComputedStyle(el).display)).toBe("block");

  // 3) 悬浮在正文右侧、不遮挡正文卡片
  const tocBox = await toc.boundingBox();
  const panelBox = await panel.boundingBox();
  expect(tocBox).not.toBeNull();
  expect(panelBox).not.toBeNull();
  if (tocBox && panelBox) {
    expect(tocBox.x).toBeGreaterThanOrEqual(panelBox.x + panelBox.width);
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

  // 5) 页面滚动后目录仍在原位（悬浮而非随正文滚走）
  const scroller = page.locator(".admin-main");
  await scroller.evaluate((el) => el.scrollTo({ top: 900 }));
  await page.waitForTimeout(120);
  const tocBoxAfter = await toc.boundingBox();
  expect(Math.abs((tocBoxAfter?.y ?? 0) - (tocBox?.y ?? 0))).toBeLessThan(4);

  // 6) 滚动高亮：滚到底时最后一项（MCP 集成）为当前区块，竖线取满高
  await scroller.evaluate((el) => el.scrollTo({ top: el.scrollHeight }));
  const lastLink = page.locator('.api-toc a[href="#mcp"]');
  await expect(lastLink).toHaveClass(/is-active/);
  // 竖线高度有 0.2s 过渡，等它长到满高再断言（否则读到过渡中间值）
  await expect
    .poll(
      () =>
        lastLink.evaluate((el) => {
          const line = parseFloat(getComputedStyle(el, "::before").height);
          return line >= el.getBoundingClientRect().height - 2;
        }),
      { timeout: 2000 },
    )
    .toBe(true);

  // 7) 短视口下目录限高（100vh - 120px），底边始终不压右下角悬浮按钮组
  await page.setViewportSize({ width: 1280, height: 620 });
  await page.waitForTimeout(120);
  const shortToc = await toc.boundingBox();
  const fab = await page.locator(".fab-main").boundingBox();
  expect(shortToc).not.toBeNull();
  expect(fab).not.toBeNull();
  if (shortToc && fab) {
    expect(shortToc.y + shortToc.height).toBeLessThanOrEqual(fab.y + 1);
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

test("地区分布：省份明细走分页对话框（不再行内展开、不溢出屏幕）", async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto("/admin");

  // 1) 行内不再有 <details> 折叠面板
  await expect(page.locator(".region-detail-popover")).toHaveCount(0);
  await expect(page.locator("details")).toHaveCount(0);

  // 2) 注入一行带 34 个省份的假数据（真实仪表盘数据由 page_views 决定，
  //    这里直接验证交互模块：够多的条目应触发分页）
  await page.evaluate(() => {
    const table = document.createElement("table");
    table.className = "data-table";
    const tr = document.createElement("tr");
    const td = document.createElement("td");
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "btn btn-sm btn-ghost region-detail-trigger";
    btn.setAttribute("data-country", "中国");
    btn.setAttribute("data-region-count", "34");
    btn.textContent = "查看明细";
    const ul = document.createElement("ul");
    ul.className = "region-detail-list";
    ul.hidden = true;
    const names = Array.from({ length: 34 }, (_, i) => `省份${String(i + 1).padStart(2, "0")}`);
    names.forEach((n, i) => {
      const li = document.createElement("li");
      li.textContent = `${n}：${34 - i}`;
      ul.appendChild(li);
    });
    td.appendChild(btn);
    td.appendChild(ul);
    tr.appendChild(td);
    table.appendChild(tr);
    (document.querySelector(".admin-content") || document.body).appendChild(table);
  });

  await page.locator(".region-detail-trigger").click();
  const box = page.locator(".modal-box.region-box");
  await expect(box).toBeVisible();
  await expect(box.locator(".modal-title")).toHaveText("省份明细 · 中国");
  // 首页只放 8 条 + 分页信息 1 / 5（34 个地区、每页 8 条）
  await expect(box.locator(".region-modal-list li")).toHaveCount(8);
  await expect(box.locator(".pager-info")).toHaveText("1 / 5");

  // 3) 弹窗不溢出视口（左右上下都在屏幕内、宽度受控、留白不过多）
  const boxBox = await box.boundingBox();
  const vp = page.viewportSize()!;
  expect(boxBox).not.toBeNull();
  if (boxBox) {
    expect(boxBox.x).toBeGreaterThanOrEqual(0);
    expect(boxBox.y).toBeGreaterThanOrEqual(0);
    expect(boxBox.x + boxBox.width).toBeLessThanOrEqual(vp.width);
    expect(boxBox.y + boxBox.height).toBeLessThanOrEqual(vp.height);
    expect(boxBox.width).toBeLessThanOrEqual(520);
  }
  // 列表区高度受控（内部滚动，不用把弹窗撑到全屏）
  const listBox = await box.locator(".region-modal-list").boundingBox();
  expect(listBox!.height).toBeLessThanOrEqual(vp.height * 0.6);
  // 满页时不出现半截行（8 条要完整放下；条数更多时靠内部滚动）
  expect(
    await box.evaluate((el) => {
      const list = el.querySelector(".region-modal-list")!;
      const lb = list.getBoundingClientRect();
      return Array.from(list.querySelectorAll("li")).some(
        (li) => li.getBoundingClientRect().bottom > lb.bottom + 1,
      );
    }),
  ).toBe(false);

  // 4) 翻页：末页（第 5 页）剩 2 条，「下一页」不可用
  for (let i = 0; i < 4; i++) {
    await box.getByRole("button", { name: "下一页" }).click();
  }
  await expect(box.locator(".pager-info")).toHaveText("5 / 5");
  await expect(box.locator(".region-modal-list li")).toHaveCount(2);
  await expect(box.getByRole("button", { name: "下一页" })).toBeDisabled();

  // 5) Esc 关闭
  await page.keyboard.press("Escape");
  await expect(box).toHaveCount(0);
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
