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
  await page.locator('[data-testid="connection-state"][data-phase="connected"]').waitFor({
    state: "visible",
    timeout: 15_000,
  });
  await page.getByTestId("audio-controls").waitFor({ state: "visible" });
  await page.getByRole("button", { name: "Follow default input" }).click();
  await page.getByRole("button", { name: "Start capture" }).click();
  try {
    await page.waitForFunction(() =>
      document.querySelector('[data-testid="audio-runtime-status"]')?.textContent?.includes("state=Running"),
      undefined,
      { timeout: 30_000 },
    );
  } catch (error) {
    const status = await page.getByTestId("audio-runtime-status").textContent();
    throw new Error(`native capture did not reach Running: ${status}\nhost stderr:\n${stderr.join("\n")}`, { cause: error });
  }

  await page.getByTestId("load-demo").click();
  await page.getByTestId("empty-workspace").waitFor({ state: "detached" });
  await page.getByRole("button", { name: "Capture", exact: true }).click();
  await waitForFrames(page, "meter-canvas-frames");
  await page.waitForTimeout(500);
  await page.getByTestId("tab-Waveform").click();
  await waitForFrames(page, "waveform-canvas-frames");
  await page.waitForTimeout(500);
  await page.getByRole("button", { name: "Transcribe", exact: true }).click();
  await page.getByTestId("tab-Spectrum").click();
  await waitForFrames(page, "spectrum-canvas-frames");
  await page.getByRole("button", { name: "Diagnose", exact: true }).click();
  await page.getByTestId("diagnostics-events").waitFor({ state: "visible" });
  await page.waitForFunction(() =>
    document.querySelector('[data-testid="diagnostics-events"]')?.textContent?.includes("native queue="),
    undefined,
    { timeout: 15_000 },
  );

  for (let transition = 0; transition < 10; transition += 1) {
    await page.getByRole("button", { name: "Transcribe", exact: true }).click();
    await page.getByTestId("tab-Spectrum").click();
    await waitForFrames(page, "spectrum-canvas-frames");
    await page.getByRole("button", { name: "Diagnose", exact: true }).click();
    await page.getByTestId("diagnostics-tile").waitFor({ state: "visible" });
  }

  await page.getByRole("button", { name: "Close Diagnostics" }).click();
  await page.getByRole("button", { name: "Reopen", exact: true }).click();
  await page.getByTestId("diagnostics-tile").waitFor({ state: "visible" });
  console.log("LIVE_OBSERVATION meter=ok waveform=ok spectrum=ok diagnostics=ok transitions=10 lease_reopen=ok");
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

async function waitForFrames(page, testId) {
  await page.getByTestId(testId).waitFor({ state: "visible" });
  try {
    await page.waitForFunction(
      (id) => /^[1-9][0-9]* frames$/.test(document.querySelector(`[data-testid="${id}"]`)?.textContent ?? ""),
      testId,
      { timeout: 30_000 },
    );
  } catch (error) {
    const lease = await page.getByTestId(testId.replace("-frames", "-lease")).textContent();
    const frames = await page.getByTestId(testId).textContent();
    throw new Error(`${testId} did not receive native frames: frames=${frames} lease=${lease}`, { cause: error });
  }
}
