import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    include: ["npm/**/*.test.ts"],
    fileParallelism: false,
  },
});
