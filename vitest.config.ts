import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import path from "node:path";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./tests/unit/setup.ts"],
    globals: true,
    // Scope to tests/unit only — tests/e2e/*.spec.ts is a separate
    // WebdriverIO suite (T049) that vitest must not try to collect.
    include: ["tests/unit/**/*.test.{ts,tsx}"],
  },
});
