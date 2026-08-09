import { spawn, execSync } from "node:child_process";
import {
  existsSync,
  rmSync,
  mkdirSync,
  writeFileSync,
  openSync,
  writeSync,
} from "node:fs";
import path from "node:path";

/**
 * 全局 setup：
 * 1. 重建独立数据目录 data-e2e/（含 config.toml + 默认主题副本）
 * 2. cargo build（有缓存时很快）后直接 spawn 二进制（便于精确 kill PID）
 * 3. 轮询 /api/health 就绪
 * 4. 走 /admin/setup 设测试密码（e2e-password-123）
 */

const REPO_ROOT = path.resolve(__dirname, "..");
const E2E_DATA = path.join(REPO_ROOT, "data-e2e");
const PORT = 8095;
const BASE = `http://127.0.0.1:${PORT}`;
const PASSWORD = "e2e-password-123";

function sleep(ms: number) {
  return new Promise((r) => setTimeout(r, ms));
}

export default async function globalSetup() {
  console.log(`[e2e-setup] 数据目录: ${E2E_DATA}`);

  // 清理可能残留的 8095 端口进程（上次失败运行遗留），避免误连旧实例
  try {
    execSync(`lsof -ti tcp:${PORT} | xargs kill -9`, { stdio: "ignore" });
  } catch {
    /* 无残留进程则忽略 */
  }

  // 1. 重建数据目录 + 主题副本
  rmSync(E2E_DATA, { recursive: true, force: true });
  mkdirSync(E2E_DATA, { recursive: true });
  writeFileSync(
    path.join(E2E_DATA, "config.toml"),
    [
      'host = "127.0.0.1"',
      `port = ${PORT}`,
      'data_dir = "data-e2e"',
      'site_name = "寒蝉 Hancic E2E"',
      'site_desc = "端到端测试站点"',
      'active_theme = "default"',
      "image_compress = true",
      "image_max_edge = 2000",
      "image_quality = 85",
      "upload_max_image = 10485760",
      "upload_max_video = 104857600",
      "upload_max_file = 52428800",
      "",
    ].join("\n"),
  );
  execSync(`cp -R themes ${E2E_DATA}/themes`, {
    cwd: REPO_ROOT,
    stdio: "ignore",
  });

  // 2. 编译（有缓存时为增量/无操作）并直接 spawn 二进制（精确 PID 便于 teardown）
  console.log("[e2e-setup] cargo build ...");
  execSync("cargo build", { cwd: REPO_ROOT, stdio: "inherit" });
  const bin = path.join(REPO_ROOT, "target", "debug", "hancic");
  if (!existsSync(bin)) {
    throw new Error(`未找到编译产物: ${bin}`);
  }
  const log = path.join(E2E_DATA, "server.log");
  const logFd = openSync(log, "w");
  const server = spawn(bin, ["data-e2e/config.toml"], {
    cwd: REPO_ROOT,
    stdio: ["ignore", "pipe", "pipe"],
  });
  server.stdio[1].on("data", (d) => writeSync(logFd, d));
  server.stdio[2].on("data", (d) => writeSync(logFd, d));
  server.on("error", (err) => {
    console.error("[e2e-setup] server spawn 失败:", err);
  });
  writeFileSync(path.join(E2E_DATA, "server.pid"), String(server.pid));

  // 3. 轮询健康检查
  let ready = false;
  for (let i = 0; i < 120; i++) {
    if (server.exitCode !== null) {
      throw new Error(`server 提前退出 code=${server.exitCode}，见 ${log}`);
    }
    try {
      const res = await fetch(`${BASE}/api/health`);
      if (res.ok) {
        ready = true;
        break;
      }
    } catch {
      /* 未就绪 */
    }
    await sleep(500);
  }
  if (!ready) {
    throw new Error(`health 检查超时（${BASE}），见 ${log}`);
  }
  console.log("[e2e-setup] server 就绪");

  // 4. 首启设置密码（setup 页带 CSRF，需保持会话 cookie）
  const cookieJar: string[] = [];
  const setupRes = await fetch(`${BASE}/admin/setup`, { redirect: "manual" });
  const setCookies = setupRes.headers.getSetCookie
    ? setupRes.headers.getSetCookie()
    : [];
  for (const c of setCookies) cookieJar.push(c.split(";")[0]);
  const html = await setupRes.text();
  const csrf = html.match(/name="csrf" value="([^"]+)"/)?.[1];
  if (!csrf) {
    throw new Error(`/admin/setup 未找到 csrf 字段，HTML 片段: ${html.slice(0, 200)}`);
  }
  const post = await fetch(`${BASE}/admin/setup`, {
    method: "POST",
    redirect: "manual",
    headers: {
      "content-type": "application/x-www-form-urlencoded",
      cookie: cookieJar.join("; "),
    },
    body: `csrf=${encodeURIComponent(csrf)}&password=${encodeURIComponent(PASSWORD)}`,
  });
  if (post.status !== 302 || post.headers.get("location") !== "/admin") {
    throw new Error(
      `setup 未按预期完成: status=${post.status} location=${post.headers.get("location")}`,
    );
  }
  console.log("[e2e-setup] 密码已设置，setup 完成");
}

// 供 teardown 使用
export { BASE, PASSWORD, E2E_DATA, REPO_ROOT };
