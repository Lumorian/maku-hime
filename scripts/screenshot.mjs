#!/usr/bin/env node
// 开发工具：用 headless Chrome 给页面截图，等真实时间让 WebSocket 数据流进页面。
// 用法: node scripts/screenshot.mjs <url> <out.png> [等待毫秒] [宽] [高] [--transparent]
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import WebSocket from "ws";

const [url, out, waitRaw, widthRaw, heightRaw] = process.argv.slice(2);
if (!url || !out) {
  console.error("用法: node scripts/screenshot.mjs <url> <out.png> [等待毫秒] [宽] [高]");
  process.exit(1);
}
const wait = Number(waitRaw ?? 6000);
const width = Number(widthRaw ?? 1440);
const height = Number(heightRaw ?? 900);
const transparent = process.argv.includes("--transparent");
const chrome = process.env.CHROME_BIN ?? "/usr/sbin/google-chrome-stable";
const profile = resolve(`.cache/chrome-shot/${process.pid}`);

mkdirSync(dirname(resolve(out)), { recursive: true });

const proc = spawn(
  chrome,
  [
    "--headless=new",
    "--no-sandbox",
    "--disable-gpu",
    "--hide-scrollbars",
    "--no-first-run",
    "--disable-dev-shm-usage",
    "--remote-debugging-port=0",
    `--user-data-dir=${profile}`,
    `--window-size=${width},${height}`,
    ...(transparent ? ["--default-background-color=00000000"] : []),
    "about:blank",
  ],
  { stdio: ["ignore", "ignore", "pipe"] },
);

const endpoint = await new Promise((resolvePromise, reject) => {
  let buffer = "";
  const timer = setTimeout(() => reject(new Error("等待 DevTools 超时")), 15000);
  proc.stderr.on("data", (chunk) => {
    buffer += chunk.toString();
    const match = buffer.match(/DevTools listening on (ws:\/\/\S+)/);
    if (match) {
      clearTimeout(timer);
      resolvePromise(match[1]);
    }
  });
});

const socket = new WebSocket(endpoint, { maxPayload: 256 * 1024 * 1024 });
let nextId = 0;
const pending = new Map();

socket.on("message", (raw) => {
  const message = JSON.parse(raw.toString());
  if (message.id && pending.has(message.id)) {
    const { resolve: done, reject } = pending.get(message.id);
    pending.delete(message.id);
    if (message.error) reject(new Error(`${methodNameById.get(message.id) ?? "?"}: ${JSON.stringify(message.error)}`));
    else done(message.result);
  }
});

await new Promise((done) => socket.on("open", done));

const methodNameById = new Map();

function send(method, params = {}, sessionId) {
  const id = ++nextId;
  methodNameById.set(id, method);
  return new Promise((done, reject) => {
    pending.set(id, { resolve: done, reject });
    socket.send(JSON.stringify({ id, method, params, sessionId }));
  });
}

const { targetId } = await send("Target.createTarget", { url: "about:blank" });
const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
await send("Page.enable", {}, sessionId);
await send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false }, sessionId);
if (transparent) {
  await send("Emulation.setDefaultBackgroundColorOverride", { color: { r: 0, g: 0, b: 0, a: 0 } }, sessionId);
}
await send("Runtime.enable", {}, sessionId);
await send("Page.navigate", { url }, sessionId);
await new Promise((done) => setTimeout(done, wait));

// 可选：截图前执行一段 JS（例如点开设置抽屉）
const evalIndex = process.argv.indexOf("--eval");
if (evalIndex !== -1) {
  const expression = process.argv[evalIndex + 1];
  const result = await send("Runtime.evaluate", { expression, awaitPromise: true }, sessionId);
  if (result?.exceptionDetails) {
    console.error("页面脚本执行失败:", result.exceptionDetails.text);
  }
  await new Promise((done) => setTimeout(done, 1500));
}

const { data } = await send("Page.captureScreenshot", { format: "png", captureBeyondViewport: false }, sessionId);
writeFileSync(resolve(out), Buffer.from(data, "base64"));
console.log(`截图已保存: ${out} (${width}×${height}, 等待 ${wait}ms)`);

socket.close();
proc.kill("SIGTERM");
process.exit(0);
