import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";

/**
 * 服务端的访问令牌文件位置（与 Rust 端 auth::token_path 保持一致：
 * dirs::config_dir()/maku-hime/api-token）。
 *
 * 开发时页面是从 5173 发起的，浏览器里没有服务端注入的令牌，
 * 所以由代理在转发时补上请求头 —— 生产代码一律不带这个变量。
 */
function devToken(): string | null {
  const fromEnv = process.env.MAKUHIME_API_TOKEN;
  if (fromEnv && fromEnv.length >= 16) return fromEnv;
  const configHome = process.env.XDG_CONFIG_HOME || join(homedir(), ".config");
  try {
    const token = readFileSync(join(configHome, "maku-hime", "api-token"), "utf8").trim();
    return token.length >= 16 ? token : null;
  } catch {
    return null;
  }
}

// 多页构建：index.html 是控制面板，obs.html 是弹幕墙（悬浮窗 / OBS 浏览器源共用）
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2021",
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        obs: resolve(__dirname, "obs.html"),
      },
    },
  },
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
    // 开发时把接口反向代理到 Rust 服务，前端代码里一律用相对路径
    proxy: {
      "/api": {
        target: "http://127.0.0.1:3939",
        changeOrigin: true,
        ws: true,
        configure: (proxy) => {
          const addToken = (proxyReq: { setHeader: (name: string, value: string) => void }) => {
            const token = devToken();
            if (token) proxyReq.setHeader("x-maku-token", token);
          };
          proxy.on("proxyReq", addToken);
          proxy.on("proxyReqWs", addToken);
        },
      },
    },
  },
  envPrefix: ["VITE_", "TAURI_"],
});
