#!/usr/bin/env node
// 开发工具：把直播间真实的弹幕流录制为 JSONL 夹具，供 Rust 解析器单元测试与人工比对使用。
// 用法: node scripts/capture.mjs <room_id> [seconds] [out.jsonl]
import { createWriteStream, mkdirSync } from "node:fs";
import crypto from "node:crypto";
import { dirname } from "node:path";
import zlib from "node:zlib";
import WebSocket from "ws";

const UA =
  "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0 Safari/537.36";
const roomArg = process.argv[2] ?? "7734200";
const seconds = Number(process.argv[3] ?? 60);
const out = process.argv[4] ?? `fixtures/live-${roomArg}.jsonl`;

async function getJSON(url) {
  const res = await fetch(url, { headers: { "User-Agent": UA, Referer: "https://live.bilibili.com/" } });
  if (!res.ok) throw new Error(`HTTP ${res.status} ${url}`);
  return res.json();
}

const spi = await getJSON("https://api.bilibili.com/x/frontend/finger/spi");
const buvid = spi?.data?.b_3 ?? "";

const info = await getJSON(`https://api.live.bilibili.com/room/v1/Room/get_info?room_id=${roomArg}`);
if (info.code !== 0) throw new Error(`room info: ${JSON.stringify(info)}`);
const roomId = info.data.room_id;
console.log(`room ${roomArg} -> ${roomId} "${info.data.title}" live_status=${info.data.live_status}`);

// getDanmuInfo 现在要求 wbi 签名，否则返回 -352 风控
const MIXIN_TAB = [46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19, 29, 28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4, 22, 25, 54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52];
const nav = await getJSON("https://api.bilibili.com/x/web-interface/nav");
if (!nav?.data?.wbi_img) throw new Error("wbi keys unavailable: " + JSON.stringify(nav));
const wbiKey =
  nav.data.wbi_img.img_url.split("/").pop().replace(".png", "") +
  nav.data.wbi_img.sub_url.split("/").pop().replace(".png", "");
const mixin = MIXIN_TAB.map((i) => wbiKey[i]).join("").slice(0, 32);

function wbiSign(url) {
  const u = new URL(url);
  const params = [...u.searchParams.entries()].sort(([a], [b]) => (a < b ? -1 : 1));
  const filtered = params.map(([k, v]) => [k, v.replace(/[!'()*]/g, "")]);
  filtered.push(["wts", String(Math.floor(Date.now() / 1000))]);
  const query = filtered.map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(v)}`).join("&");
  const w_rid = crypto.createHash("md5").update(query + mixin).digest("hex");
  return `${u.origin}${u.pathname}?${query}&w_rid=${w_rid}`;
}

const danmu = await getJSON(
  wbiSign(`https://api.live.bilibili.com/xlive/web-room/v1/index/getDanmuInfo?id=${roomId}&type=0`),
);
if (danmu.code !== 0) throw new Error(`danmu info: ${JSON.stringify(danmu)}`);
const token = danmu.data.token;
const host = danmu.data.host_list[0];

function encode(op, body) {
  const payload = Buffer.isBuffer(body) ? body : Buffer.from(body, "utf8");
  const buf = Buffer.alloc(16 + payload.length);
  buf.writeUInt32BE(16 + payload.length, 0);
  buf.writeUInt16BE(16, 4);
  buf.writeUInt16BE(1, 6);
  buf.writeUInt32BE(op, 8);
  buf.writeUInt32BE(1, 12);
  payload.copy(buf, 16);
  return buf;
}

const counts = new Map();
let total = 0;
let ws;

mkdirSync(dirname(out), { recursive: true });
const file = createWriteStream(out, { flags: "w" });

function handlePacket(buf) {
  if (buf.length < 16) return;
  const totalLen = buf.readUInt32BE(0);
  const headerLen = buf.readUInt16BE(4);
  const proto = buf.readUInt16BE(6);
  const op = buf.readUInt32BE(8);
  const payload = buf.subarray(headerLen, Math.min(totalLen, buf.length));
  if (proto === 2) {
    try { handlePacket(zlib.inflateSync(payload)); } catch (e) { console.warn("zlib", e.message); }
    return;
  }
  if (proto === 3) {
    try { handlePacket(zlib.brotliDecompressSync(payload)); } catch (e) { console.warn("brotli", e.message); }
    return;
  }
  if (op === 5) {
    const text = payload.toString("utf8");
    let msg;
    try { msg = JSON.parse(text); } catch { return; }
    const cmd = String(msg.cmd ?? "?").split(":")[0];
    counts.set(cmd, (counts.get(cmd) ?? 0) + 1);
    total += 1;
    file.write(JSON.stringify(msg) + "\n");
    return;
  }
  if (op === 3) {
    const popularity = payload.length >= 4 ? payload.readUInt32BE(0) : 0;
    console.log("heartbeat reply, popularity =", popularity);
    return;
  }
  if (op === 8) {
    console.log("auth reply:", payload.toString("utf8"));
  }
}

ws = new WebSocket(`wss://${host.host}:${host.wss_port}/sub`, {
  headers: {
    Origin: "https://live.bilibili.com",
    "User-Agent": UA,
    Referer: `https://live.bilibili.com/${roomId}`,
  },
});
ws.binaryType = "nodebuffer";

const heartbeat = setInterval(() => {
  if (ws.readyState === WebSocket.OPEN) ws.send(encode(2, "[object Object]"));
}, 30_000);

ws.on("open", () => {
  console.log("ws open", host.host);
  ws.send(
    encode(
      7,
      JSON.stringify({ uid: 0, roomid: roomId, protover: 3, buvid, platform: "web", type: 2, key: token }),
    ),
  );
});
ws.on("message", (data) => handlePacket(Buffer.from(data)));
ws.on("close", (code, reason) => console.log("ws close", code, reason.toString()));
ws.on("error", (e) => console.error("ws error", e.message));

setTimeout(() => {
  clearInterval(heartbeat);
  ws.close();
  file.end(() => {
    console.log(`\ncaptured ${total} messages in ${seconds}s -> ${out}`);
    for (const [cmd, n] of [...counts.entries()].sort((a, b) => b[1] - a[1])) {
      console.log(`  ${cmd.padEnd(32)} ${n}`);
    }
    process.exit(0);
  });
}, seconds * 1000);
