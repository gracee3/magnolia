import { expect, test as base, type Page } from "@playwright/test";
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { access } from "node:fs/promises";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";

const repositoryRoot = fileURLToPath(new URL("../..", import.meta.url));
const authorityHeader = "x-magnolia-test-authority";

export interface HostInfo {
  origin: string;
  launchUrl: string;
  runtimeEpoch: string;
  testAuthority: string;
  process: ChildProcessWithoutNullStreams;
  stderr: string[];
}

export interface HostStatus {
  projection: {
    runtime_epoch: string;
    revision: number;
    document_revision: number;
    target_graph_revision: number;
    active_graph_revision: number;
    workspace: {
      graph: {
        modules: Record<string, unknown>;
        edges: Record<string, unknown>;
      };
    };
    operations: Array<{
      target_graph_revision: number;
      state: "pending" | "succeeded" | "failed" | "superseded";
    }>;
    transcript: {
      revision: number;
      final_segment_count: number;
    };
  };
  pending_activations: number;
  observed_activations: number;
  telemetry: {
    active_connections: number;
    total_connections: number;
    active_leases: number;
    released_leases: number;
    cumulative_dropped: number;
    flood_multiplier: number;
  };
}

export interface AppFixture {
  page: Page;
  consoleErrors: string[];
}

interface Fixtures {
  host: HostInfo;
  app: AppFixture;
}

export const test = base.extend<Fixtures>({
  host: async ({}, use) => {
    const host = await startHost();
    try {
      await use(host);
    } finally {
      await stopHost(host);
    }
  },
  app: async ({ host, page }, use, testInfo) => {
    const consoleErrors: string[] = [];
    page.on("console", (message) => {
      if (message.type() === "error") {
        consoleErrors.push(`console: ${message.text()}`);
      }
    });
    page.on("pageerror", (error) => consoleErrors.push(`page: ${error.stack ?? error.message}`));
    page.on("response", (response) => {
      if (response.status() >= 400) {
        consoleErrors.push(`http ${response.status()}: ${response.url()}`);
      }
    });
    await page.addInitScript(() => {
      Error.stackTraceLimit = 100;
    });
    await page.goto(host.launchUrl, { waitUntil: "domcontentloaded" });
    await expect(page.getByTestId("studio-shell")).toBeVisible();
    await expect(page.getByTestId("connection-state")).toHaveAttribute("data-phase", "connected");
    await expect(page.getByTestId("connection-state")).toContainText("protocol 1.0");
    expect(page.url()).not.toContain("token=");
    await use({ page, consoleErrors });
    if (testInfo.status !== testInfo.expectedStatus) {
      await testInfo.attach("native-host-stderr", {
        body: host.stderr.join("\n"),
        contentType: "text/plain",
      });
    }
  },
});

export { expect };

async function startHost(): Promise<HostInfo> {
  const binary = process.env.MAGNOLIA_DESKTOP_BIN ?? resolve(repositoryRoot, "target/phase2/debug/magnolia-desktop");
  const assets = process.env.MAGNOLIA_WEB_ASSETS ?? resolve(repositoryRoot, "target/magnolia-studio-web-dist");
  await access(binary).catch(() => {
    throw new Error(`missing native test host: ${binary}; run the Phase 2 build gate first`);
  });
  await access(resolve(assets, "index.html")).catch(() => {
    throw new Error(`missing Trunk assets: ${assets}; run the Phase 2 build gate first`);
  });

  const child = spawn(binary, ["--assets", assets, "--no-browser", "--test-mode"], {
    cwd: repositoryRoot,
    env: { ...process.env, RUST_LOG: "warn" },
    stdio: ["pipe", "pipe", "pipe"],
  });
  const stderr: string[] = [];
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk: string) => stderr.push(chunk.trimEnd()));

  const ready = new Promise<Omit<HostInfo, "process" | "stderr">>((resolveReady, reject) => {
    const lines = createInterface({ input: child.stdout });
    const timer = setTimeout(() => reject(new Error(`native host did not become ready\n${stderr.join("\n")}`)), 15_000);
    lines.on("line", (line) => {
      if (!line.startsWith("MAGNOLIA_READY ")) {
        return;
      }
      clearTimeout(timer);
      const parsed = JSON.parse(line.slice("MAGNOLIA_READY ".length)) as {
        origin: string;
        launch_url: string;
        runtime_epoch: string;
        test_authority: string;
      };
      resolveReady({
        origin: parsed.origin,
        launchUrl: parsed.launch_url,
        runtimeEpoch: parsed.runtime_epoch,
        testAuthority: parsed.test_authority,
      });
    });
    child.once("exit", (code) => {
      clearTimeout(timer);
      reject(new Error(`native host exited before readiness with ${code}\n${stderr.join("\n")}`));
    });
  });
  const info = await ready;
  const host = { ...info, process: child, stderr };
  await expect.poll(async () => (await fetch(`${host.origin}/api/health`)).status).toBe(200);
  return host;
}

async function stopHost(host: HostInfo): Promise<void> {
  if (host.process.exitCode !== null) {
    return;
  }
  host.process.kill("SIGINT");
  const exited = new Promise<boolean>((resolveExit) => host.process.once("exit", () => resolveExit(true)));
  const timedOut = new Promise<boolean>((resolveTimeout) => setTimeout(() => resolveTimeout(false), 5_000));
  if (!(await Promise.race([exited, timedOut])) && host.process.exitCode === null) {
    host.process.kill("SIGKILL");
  }
}

export async function status(host: HostInfo): Promise<HostStatus> {
  const response = await fetch(`${host.origin}/__test/status`, {
    headers: { [authorityHeader]: host.testAuthority },
  });
  expect(response.status).toBe(200);
  return await response.json() as HostStatus;
}

export async function runtimeAction(
  host: HostInfo,
  action: "succeed_next" | "fail_next" | "succeed_target" | "pump",
  targetRevision?: number,
): Promise<{ completed_target: number | null; handled: number; ignored_stale: number }> {
  const response = await fetch(`${host.origin}/__test/runtime`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      [authorityHeader]: host.testAuthority,
    },
    body: JSON.stringify({ action, target_revision: targetRevision ?? null }),
  });
  expect(response.status).toBe(200);
  return await response.json() as { completed_target: number | null; handled: number; ignored_stale: number };
}

export async function setFlood(host: HostInfo, floodMultiplier: number): Promise<void> {
  const response = await fetch(`${host.origin}/__test/telemetry`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      [authorityHeader]: host.testAuthority,
    },
    body: JSON.stringify({ flood_multiplier: floodMultiplier }),
  });
  expect(response.status).toBe(204);
}

export async function disconnectBrowser(host: HostInfo): Promise<void> {
  const response = await fetch(`${host.origin}/__test/disconnect`, {
    method: "POST",
    headers: { [authorityHeader]: host.testAuthority },
  });
  expect(response.status).toBe(204);
}

export async function loadDemo(page: Page): Promise<void> {
  await page.getByTestId("load-demo").click();
  await expect(page.getByTestId("document-revision")).toHaveText("1");
  await expect(page.getByTestId("target-revision")).toHaveText("1");
  await expect(page.getByTestId("receipt-status")).toContainText("Accepted");
  await expect(page.getByTestId("graph-patch")).toBeVisible();
}

export async function activateInitialGraph(host: HostInfo, page: Page): Promise<void> {
  await runtimeAction(host, "succeed_next");
  await expect(page.getByTestId("active-revision")).toHaveText("1");
}

export async function commitLayout(page: Page): Promise<void> {
  await page.getByTestId("preview-layout").click();
  await expect(page.getByTestId("layout-draft")).toBeVisible();
  await page.getByTestId("commit-layout").click();
  await expect(page.getByTestId("layout-draft")).toBeHidden();
}
