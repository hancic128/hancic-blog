import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { execSync } from "node:child_process";

/** 全局 teardown：kill global-setup 启动的 dev server（按 PID 文件），保留 data-e2e 供排障。 */
export default async function globalTeardown() {
  const pidFile = path.resolve(__dirname, "..", "data-e2e", "server.pid");
  if (!existsSync(pidFile)) {
    console.log("[e2e-teardown] 无 PID 文件，跳过");
    return;
  }
  const pid = readFileSync(pidFile, "utf-8").trim();
  try {
    process.kill(Number(pid), "SIGTERM");
    // 等待退出
    const deadline = Date.now() + 5000;
    while (Date.now() < deadline) {
      try {
        process.kill(Number(pid), 0);
        await new Promise((r) => setTimeout(r, 100));
      } catch {
        break; // 进程已退出
      }
    }
    console.log(`[e2e-teardown] server(${pid}) 已停止`);
  } catch (err) {
    console.log(`[e2e-teardown] server(${pid}) 已不在运行: ${err}`);
  }
  // 兜底：确保端口释放
  try {
    execSync(`lsof -ti tcp:8095 | xargs kill -9`, { stdio: "ignore" });
  } catch {
    /* ignore */
  }
}
