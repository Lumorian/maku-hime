import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";

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
      },
    },
  },
  envPrefix: ["VITE_", "TAURI_"],
});
