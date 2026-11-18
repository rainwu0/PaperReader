import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// 手機上載入的是打包後的靜態檔，不跑開發伺服器。
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { target: "es2022" },
});
