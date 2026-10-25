#!/usr/bin/env node
// 从录制的真实弹幕里裁一份测试夹具（各类消息各取几条 + 几条合成样例）。
// 用法: node scripts/make-fixture.mjs [源文件] [输出文件]
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";

const src = process.argv[2] ?? "fixtures/live-7734200.jsonl";
const out = process.argv[3] ?? "crates/makuhime-core/tests/fixtures/messages.jsonl";

const wanted = {
  DANMU_MSG: 3,
  SEND_GIFT_V2: 2,
  INTERACT_WORD_V2: 2,
  LIKE_INFO_V3_CLICK: 1,
  LIKE_INFO_V3_UPDATE: 1,
  WATCHED_CHANGE: 1,
  ONLINE_RANK_V3: 1,
};

const picked = [];
for (const line of readFileSync(src, "utf8").split("\n")) {
  if (!line.trim()) continue;
  let message;
  try {
    message = JSON.parse(line);
  } catch {
    continue;
  }
  const cmd = String(message.cmd ?? "").split(":")[0];
  if (wanted[cmd] > 0) {
    picked.push(line.trim());
    wanted[cmd] -= 1;
  }
}

const synthetic = [
  JSON.stringify({
    cmd: "SUPER_CHAT_MESSAGE",
    data: {
      id: 9001,
      uid: 123456,
      price: 30,
      message: "这波操作太顶了",
      time: 60,
      background_color_start: "#EDF5FF",
      background_color_end: "#DBE7FF",
      background_price_color: "#7EA6E0",
      user_info: {
        uname: "老板来了",
        face: "https://i0.hdslb.com/bfs/face/sample.jpg",
        user_level: 22,
        guard_level: 3,
      },
      medal_info: {
        medal_name: "会考上",
        medal_level: 14,
        medal_color_start: "#5C968E",
        medal_color_end: "#5C968E",
        medal_color_border: "#5C968E",
        guard_level: 0,
        is_light: 1,
      },
    },
  }),
  JSON.stringify({
    cmd: "GUARD_BUY",
    data: {
      uid: 654321,
      username: "新舰长",
      guard_level: 3,
      num: 1,
      price: 198000,
      gift_name: "舰长",
    },
  }),
  JSON.stringify({
    cmd: "SEND_GIFT",
    data: {
      uid: 111,
      uname: "老式礼物",
      face: "https://i0.hdslb.com/bfs/face/sample2.jpg",
      giftName: "辣条",
      giftId: 1,
      num: 5,
      price: 100,
      total_coin: 500,
      coin_type: "gold",
      action: "投喂",
      batch_combo_id: "batch:gift:combo_id:1:2:1:1700000000.1",
    },
  }),
  JSON.stringify({ cmd: "LIVE", data: { roomid: 7734200 } }),
  JSON.stringify({ cmd: "PREPARING", data: { roomid: 7734200 } }),
];

mkdirSync(dirname(resolve(out)), { recursive: true });
writeFileSync(resolve(out), [...picked, ...synthetic].join("\n") + "\n");
console.log(`夹具已写入 ${out}: 真实 ${picked.length} 条 + 合成 ${synthetic.length} 条`);
