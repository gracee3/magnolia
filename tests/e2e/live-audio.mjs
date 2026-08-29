import { chromium } from "@playwright/test";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";

const binary = process.env.MAGNOLIA_DESKTOP_BIN;
const assets = process.env.MAGNOLIA_WEB_ASSETS;
const executablePath = process.env.MAGNOLIA_CHROMIUM;
if (!binary || !assets || !executablePath) {
  throw new Error("MAGNOLIA_DESKTOP_BIN, MAGNOLIA_WEB_ASSETS, and MAGNOLIA_CHROMIUM are required");
}

const host = spawn(binary, ["--assets", assets, "--no-browser"], {
  stdio: ["ignore", "pipe", "pipe"],
  env: { ...process.env, RUST_LOG: "warn" },
});
const stderr = [];
host.stderr.setEncoding("utf8");
host.stderr.on("data", (chunk) => stderr.push(chunk.trimEnd()));

const launchUrl = await new Promise((resolve, reject) => {
  const lines = createInterface({ input: host.stdout });
  const timer = setTimeout(() => reject(new Error(`native host readiness timed out\n${stderr.join("\n")}`)), 15_000);
  lines.on("line", (line) => {
    const prefix = "Open this local URL manually: ";
    if (line.startsWith(prefix)) {
      clearTimeout(timer);
      resolve(line.slice(prefix.length));
    }
  });
  host.once("exit", (code) => {
    clearTimeout(timer);
    reject(new Error(`native host exited before readiness with ${code}\n${stderr.join("\n")}`));
  });
});

let browser;
try {
  browser = await chromium.launch({ executablePath, headless: true, args: ["--no-sandbox"] });
  const context = await browser.newContext();
  const page = await context.newPage();
  await page.goto(launchUrl, { waitUntil: "domcontentloaded" });
  await page.getByTestId("connection-state").waitFor({ state: "visible" });
  await page.getByTestId("audio-controls").waitFor({ state: "visible" });
  await page.getByRole("button", { name: "Follow default input" }).click();
  await page.getByTestId("active-revision").waitFor({ state: "visible" });
  await page.getByRole("button", { name: "Start capture" }).click();
  await waitForRunning(page);

  let previousCallbacks = callbackCount(await page.getByTestId("audio-runtime-status").textContent());
  for (let reload = 1; reload <= 20; reload += 1) {
    await page.reload({ waitUntil: "domcontentloaded" });
    await waitForRunning(page);
    const callbacks = callbackCount(await page.getByTestId("audio-runtime-status").textContent());
    if (callbacks < previousCallbacks) {
      throw new Error(`callback count regressed across reload ${reload}: ${callbacks} < ${previousCallbacks}`);
    }
    previousCallbacks = callbacks;
  }

  await page.getByRole("button", { name: "Stop", exact: true }).click();
  await page.getByTestId("audio-runtime-status").waitFor({ state: "visible" });
  await page.waitForFunction(() => document.querySelector('[data-testid="audio-runtime-status"]')?.textContent?.includes("state=Stopped"));
  console.log(`LIVE_BROWSER reloads=20 callbacks=${previousCallbacks} state=Stopped`);
  await context.close();
} finally {
  await browser?.close();
  if (host.exitCode === null) {
    host.kill("SIGINT");
    await Promise.race([
      new Promise((resolve) => host.once("exit", resolve)),
      new Promise((_, reject) => setTimeout(() => reject(new Error("native host teardown timed out")), 5_000)),
    ]);
  }
}

async function waitForRunning(page) {
  await page.getByTestId("connection-state").waitFor({ state: "visible" });
  await page.getByTestId("audio-runtime-status").waitFor({ state: "visible" });
  await page.waitForFunction(() => {
    const text = document.querySelector('[data-testid="audio-runtime-status"]')?.textContent ?? "";
    return text.includes("state=Running") && /callbacks=[1-9][0-9]*/.test(text);
  }, undefined, { timeout: 15_000 });
}

function callbackCount(text) {
  const match = /callbacks=([0-9]+)/.exec(text ?? "");
  if (!match) {
    throw new Error(`callback count missing from audio projection: ${text}`);
  }
  return Number.parseInt(match[1], 10);
}
