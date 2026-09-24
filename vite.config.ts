import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri 期望一个固定端口的开发服务器，且不要在 Rust 源码变动时热重载前端。
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_"],
  // macOS 12 对应的 WKWebView 版本约为 Safari 15，必须显式降级构建目标，
  // 否则最新的 ES 语法会导致白屏（见 PRD 5.1 / 11）。
  build: {
    target: "safari15",
    minify: "esbuild",
    sourcemap: false,
    cssMinify: true,
    chunkSizeWarningLimit: 900,
  },
});
