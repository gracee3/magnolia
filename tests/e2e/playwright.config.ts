import { defineConfig, devices } from "@playwright/test";

const executablePath = process.env.MAGNOLIA_CHROMIUM;

export default defineConfig({
  testDir: ".",
  testMatch: "magnolia.spec.ts",
  outputDir: "test-results",
  fullyParallel: false,
  workers: 1,
  timeout: 120_000,
  expect: {
    timeout: 10_000,
  },
  reporter: process.env.CI ? [["line"], ["html", { open: "never" }]] : "line",
  use: {
    ...devices["Desktop Chrome"],
    headless: true,
    viewport: { width: 1440, height: 900 },
    actionTimeout: 10_000,
    navigationTimeout: 15_000,
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
    launchOptions: {
      executablePath,
      args: ["--no-sandbox"],
    },
  },
});
