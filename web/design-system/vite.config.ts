import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    restoreMocks: true,
    maxWorkers: 2,
    coverage: {
      provider: "v8",
      include: ["src/**/*.{ts,tsx}"],
      exclude: ["src/**/*.test.{ts,tsx}", "src/test/**", "src/**/*.d.ts"],
      reporter: ["json", "json-summary", "text-summary"],
      reportsDirectory: "../../quality/coverage/web-design-system",
    },
    setupFiles: "./src/test/setup.ts",
  },
});
