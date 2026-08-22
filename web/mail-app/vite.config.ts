import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    setupFiles: "./src/test/setup.ts",
    restoreMocks: true,
  },
  build: { sourcemap: true, target: "es2022" },
  server: { port: 4174, proxy: { "/api": "http://127.0.0.1:8081" } },
});
