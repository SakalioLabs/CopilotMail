import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  test: {
    // The mobile touch-target contract reads the feature stylesheets as raw
    // Vite modules. Process CSS in Vitest instead of replacing those imports
    // with an empty stub.
    css: true,
    environment: "jsdom",
    setupFiles: "./src/test/setup.ts",
    restoreMocks: true,
    maxWorkers: 2,
    coverage: {
      provider: "v8",
      allowExternal: true,
      include: ["src/**/*.{ts,tsx}", "../design-system/src/**/*.{ts,tsx}"],
      exclude: ["**/*.test.{ts,tsx}", "**/src/test/**", "**/*.d.ts"],
      reporter: ["json", "json-summary", "text-summary"],
      reportsDirectory: "../../quality/coverage/web-mail",
    },
  },
  build: { sourcemap: false, target: "es2022" },
  server: { port: 4174, proxy: { "/api": "http://127.0.0.1:8081" } },
});
