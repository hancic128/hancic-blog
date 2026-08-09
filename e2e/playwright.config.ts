import { defineConfig } from "@playwright/test";

/**
 * Hancic E2E：本地起 dev server（data-e2e 独立数据目录，端口 8095），
 * desktop / mobile 两个 project 各跑对应 spec。
 * - desktop：publish-post / publish-moment / paste-upload
 * - mobile（375x667）：mobile.spec.ts
 * 服务器生命周期由 global-setup / global-teardown 管理。
 */
export default defineConfig({
  testDir: "./tests",
  timeout: 60_000,
  // 用例间不共享状态；同一 project 内不同 spec 可并行（各自独立浏览器上下文）
  fullyParallel: false,
  // 所有 spec 串行执行：data-e2e 为共享数据目录，避免附件/文章计数相互干扰
  workers: 1,
  retries: 0,
  reporter: [["list"]],
  globalSetup: "./global-setup.ts",
  globalTeardown: "./global-teardown.ts",
  use: {
    baseURL: "http://127.0.0.1:8095",
    trace: "retain-on-failure",
  },
  projects: [
    {
      name: "desktop",
      testIgnore: /mobile\.spec\.ts/,
      use: { viewport: { width: 1280, height: 800 } },
    },
    {
      name: "mobile",
      testMatch: /mobile\.spec\.ts/,
      use: { viewport: { width: 375, height: 667 } },
    },
  ],
});
