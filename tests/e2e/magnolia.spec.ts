import {
  activateInitialGraph,
  commitLayout,
  disconnectBrowser,
  expect,
  loadDemo,
  runtimeAction,
  setFlood,
  status,
  test,
  type HostInfo,
} from "./fixtures";
import type { Page } from "@playwright/test";

const spectrumTile = "00000000-0000-0000-0000-00000002000a";

test("negotiates, edits the graph, preserves last-good, and rejects stale completion", async ({ host, app }) => {
  const { page, consoleErrors } = app;
  await expect(page.getByTestId("runtime-epoch")).toHaveText(host.runtimeEpoch);
  await expect(page.getByTestId("document-revision")).toHaveText("0");

  await loadDemo(page);
  let native = await status(host);
  expect(native.pending_activations).toBe(1);
  expect(native.observed_activations).toBe(1);
  expect(Object.keys(native.projection.workspace.graph.modules)).toHaveLength(3);
  expect(Object.keys(native.projection.workspace.graph.edges)).toHaveLength(2);

  await activateInitialGraph(host, page);
  native = await status(host);
  expect(native.projection.active_graph_revision).toBe(1);

  await setFirstGain(page, "1.2", 2);
  await runtimeAction(host, "fail_next");
  await expect(page.getByTestId("active-revision")).toHaveText("1");
  native = await status(host);
  expect(native.projection.target_graph_revision).toBe(2);
  expect(native.projection.active_graph_revision).toBe(1);
  expect(native.projection.operations.some((operation) => operation.target_graph_revision === 2 && operation.state === "failed")).toBe(true);

  await setFirstGain(page, "1.3", 3);
  await setFirstGain(page, "1.4", 4);
  await runtimeAction(host, "succeed_target", 4);
  await expect(page.getByTestId("active-revision")).toHaveText("4");
  const stale = await runtimeAction(host, "succeed_target", 3);
  expect(stale.ignored_stale).toBe(1);
  native = await status(host);
  expect(native.projection.active_graph_revision).toBe(4);
  expect(native.projection.operations.some((operation) => operation.target_graph_revision === 3 && operation.state === "superseded")).toBe(true);
  expect(consoleErrors).toEqual([]);
});

test("keeps layout drafts local, commits document-only edits, and supports pointer and keyboard focus", async ({ host, app }) => {
  const { page, consoleErrors } = app;
  await loadDemo(page);
  await activateInitialGraph(host, page);
  const before = await status(host);

  await page.getByTestId("preview-layout").click();
  await expect(page.getByTestId("layout-draft")).toBeVisible();
  let draftStatus = await status(host);
  expect(draftStatus.projection.document_revision).toBe(before.projection.document_revision);
  expect(draftStatus.projection.target_graph_revision).toBe(before.projection.target_graph_revision);
  await page.getByTestId("commit-layout").click();
  await expect(page.getByTestId("document-revision")).toHaveText("2");
  await expect(page.getByTestId("layout-draft")).toBeHidden();

  let committed = await status(host);
  expect(committed.projection.target_graph_revision).toBe(before.projection.target_graph_revision);
  expect(committed.projection.active_graph_revision).toBe(before.projection.active_graph_revision);
  expect(committed.pending_activations).toBe(0);
  expect(committed.observed_activations).toBe(before.observed_activations);

  await page.getByTestId("retry-receipt").click();
  await expect(page.getByTestId("receipt-status")).toContainText("sequence 2");
  committed = await status(host);
  expect(committed.projection.document_revision).toBe(2);
  expect(committed.observed_activations).toBe(before.observed_activations);

  await page.keyboard.press("l");
  await expect(page.getByTestId("layout-draft")).toBeVisible();
  await page.keyboard.press("Shift+l");
  await expect(page.getByTestId("document-revision")).toHaveText("3");

  const epoch = await page.getByTestId("runtime-epoch").textContent();
  await page.getByTestId("workspace-transcribe").click();
  await expect(page.getByTestId("workspace-transcribe")).toHaveClass(/active/);
  await page.keyboard.press("Alt+4");
  await expect(page.getByTestId("workspace-diagnose")).toHaveClass(/active/);
  await expect(page.getByTestId("runtime-epoch")).toHaveText(epoch ?? "");

  await page.keyboard.press("F6");
  await expect(page.getByTestId("focused-tile")).not.toHaveText("none");
  await page.getByTestId("workspace-patch").click();
  await page.getByRole("tab", { name: "Module controls" }).click();
  const textControl = page.getByRole("textbox").first();
  await textControl.fill("ordinary browser text");
  const documentBeforeTyping = await page.getByTestId("document-revision").textContent();
  await textControl.press("u");
  await expect(textControl).toHaveValue("ordinary browser textu");
  await expect(page.getByTestId("document-revision")).toHaveText(documentBeforeTyping ?? "");

  await page.keyboard.press("Control+k");
  await expect(page.getByTestId("command-palette")).toBeVisible();
  const search = page.getByTestId("command-search");
  await search.fill("undo");
  await search.press("Home");
  await search.press("x");
  await expect(search).toHaveValue("xundo");
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("command-palette")).toBeHidden();
  expect(consoleErrors).toEqual([]);
});

test("bounds binary telemetry, isolates control receipts, and releases hidden leases", async ({ host, app }) => {
  const { page, consoleErrors } = app;
  await loadDemo(page);
  await activateInitialGraph(host, page);
  await page.getByTestId("workspace-diagnose").click();
  await expect(page.getByTestId("meter-canvas")).toBeVisible();
  await expectFrameCount(page, "meter-canvas");
  await expect.poll(async () => (await status(host)).telemetry.active_leases).toBe(2);

  await page.getByRole("tab", { name: "Waveform" }).click();
  await expect(page.getByTestId("waveform-canvas")).toBeVisible();
  await expectFrameCount(page, "waveform-canvas");
  await expect.poll(async () => (await status(host)).telemetry.active_leases).toBe(2);

  await page.getByRole("tab", { name: "Spectrum" }).click();
  await expect(page.getByTestId("spectrum-canvas")).toBeVisible();
  await expectFrameCount(page, "spectrum-canvas");
  const releasesBeforeClose = (await status(host)).telemetry.released_leases;
  await page.getByRole("button", { name: "Close Spectrum" }).click();
  await expect.poll(async () => (await status(host)).telemetry.active_leases).toBe(1);
  await expect.poll(async () => (await status(host)).telemetry.released_leases).toBeGreaterThan(releasesBeforeClose);

  await page.getByTestId(`open-tile-${spectrumTile}`).click();
  await expect(page.getByTestId("spectrum-canvas")).toBeVisible();
  await expect.poll(async () => (await status(host)).telemetry.active_leases).toBe(2);
  await setFlood(host, 2_000);
  await expect.poll(async () => (await status(host)).telemetry.cumulative_dropped, { timeout: 15_000 }).toBeGreaterThan(0);

  const started = Date.now();
  await commitLayout(page);
  await expect(page.getByTestId("document-revision")).toHaveText("2");
  expect(Date.now() - started).toBeLessThan(10_000);
  const flooded = await status(host);
  expect(flooded.projection.target_graph_revision).toBe(1);
  expect(flooded.projection.active_graph_revision).toBe(1);
  expect(flooded.telemetry.cumulative_dropped).toBeGreaterThan(0);
  await setFlood(host, 1);
  expect(consoleErrors).toEqual([]);
});

test("reconnects and reloads an active stream twenty times without runtime or lease growth", async ({ host, app }) => {
  const { page, consoleErrors } = app;
  await loadDemo(page);
  await activateInitialGraph(host, page);
  await page.getByTestId("workspace-diagnose").click();
  await expectFrameCount(page, "meter-canvas");
  await expect.poll(async () => (await status(host)).telemetry.active_leases).toBe(2);
  const initial = await status(host);
  const initialGraph = JSON.stringify(initial.projection.workspace.graph);
  const initialCursor = Number(await page.evaluate(() => sessionStorage.getItem("magnolia.transcript.v1") ?? "0"));

  const connectionsBefore = initial.telemetry.total_connections;
  await disconnectBrowser(host);
  await expect(page.getByTestId("connection-state")).toHaveAttribute("data-phase", /reconnecting|disconnected/);
  const whileDisconnected = await status(host);
  expect(whileDisconnected.projection.runtime_epoch).toBe(initial.projection.runtime_epoch);
  expect(whileDisconnected.projection.active_graph_revision).toBe(1);
  await expect(page.getByTestId("connection-state")).toHaveAttribute("data-phase", "connected", { timeout: 15_000 });
  await expect.poll(async () => (await status(host)).telemetry.total_connections).toBeGreaterThan(connectionsBefore);

  for (let reload = 1; reload <= 20; reload += 1) {
    await page.reload({ waitUntil: "domcontentloaded" });
    await expect(page.getByTestId("connection-state")).toHaveAttribute("data-phase", "connected", { timeout: 15_000 });
    await expect(page.getByTestId("workspace-diagnose")).toHaveClass(/active/);
    await expect(page.getByTestId("runtime-epoch")).toHaveText(initial.projection.runtime_epoch);
    await expect.poll(async () => (await status(host)).telemetry.active_leases).toBe(2);
    if (reload === 10) {
      await page.waitForTimeout(1_100);
    }
  }

  const final = await status(host);
  expect(final.projection.runtime_epoch).toBe(initial.projection.runtime_epoch);
  expect(JSON.stringify(final.projection.workspace.graph)).toBe(initialGraph);
  expect(final.projection.active_graph_revision).toBe(1);
  expect(final.observed_activations).toBe(initial.observed_activations);
  expect(final.telemetry.active_connections).toBe(1);
  expect(final.telemetry.active_leases).toBe(2);
  const finalCursor = Number(await page.evaluate(() => sessionStorage.getItem("magnolia.transcript.v1") ?? "0"));
  expect(finalCursor).toBeGreaterThanOrEqual(initialCursor);
  expect(finalCursor).toBeLessThanOrEqual(final.projection.transcript.final_segment_count);
  expect(final.projection.transcript.final_segment_count).toBeGreaterThan(0);
  expect(unexpectedConsoleErrors(consoleErrors)).toEqual([]);
});

test("rejects invalid credentials, origins, and protocol majors", async ({ browser, host, app }) => {
  const { page, consoleErrors } = app;

  const badCredentialContext = await browser.newContext();
  const badCredentialPage = await badCredentialContext.newPage();
  await badCredentialPage.goto(`${host.origin}/#token=not-a-valid-launch-token`);
  await expect(badCredentialPage.getByTestId("connection-state")).toHaveAttribute("data-phase", "rejected");
  await badCredentialContext.close();

  const invalidOriginContext = await browser.newContext();
  const invalidOriginPage = await invalidOriginContext.newPage();
  await invalidOriginPage.goto("data:text/html,<title>invalid origin</title>");
  const originResult = await invalidOriginPage.evaluate(async (origin) => {
    const socketUrl = origin.replace("http://", "ws://") + "/api/control";
    return await new Promise<string>((resolve) => {
      const socket = new WebSocket(socketUrl);
      const timer = window.setTimeout(() => resolve("timeout"), 3_000);
      socket.onopen = () => {
        window.clearTimeout(timer);
        socket.close();
        resolve("opened");
      };
      socket.onerror = () => {
        window.clearTimeout(timer);
        resolve("rejected");
      };
    });
  }, host.origin);
  expect(originResult).toBe("rejected");
  await invalidOriginContext.close();

  const protocolResponse = await unsupportedProtocolResponse(page, host);
  expect(protocolResponse.kind).toBe("connected");
  expect(protocolResponse.response.status).toBe("rejected");
  expect(protocolResponse.response.error.code).toBe("unsupported_major");
  expect(consoleErrors).toEqual([]);
});

async function setFirstGain(page: Page, value: string, expectedTarget: number): Promise<void> {
  await page.getByRole("tab", { name: "Module controls" }).click();
  const input = page.getByRole("spinbutton").first();
  await input.fill(value);
  await input.press("Tab");
  await expect(page.getByTestId("target-revision")).toHaveText(expectedTarget.toString());
  await expect(page.getByTestId("receipt-status")).toContainText("Accepted");
}

async function expectFrameCount(page: Page, testId: string): Promise<void> {
  await expect.poll(async () => {
    const text = await page.getByTestId(`${testId}-frames`).textContent();
    return Number.parseInt(text ?? "0", 10);
  }).toBeGreaterThan(0);
}

function unexpectedConsoleErrors(errors: string[]): string[] {
  return errors.filter((error) => !error.includes("WebSocket connection") && !error.includes("ERR_INTERNET_DISCONNECTED"));
}

async function unsupportedProtocolResponse(page: Page, host: HostInfo): Promise<any> {
  return await page.evaluate(async ({ origin, runtimeEpoch }) => {
    const sessionId = sessionStorage.getItem("magnolia.session.v1");
    const clientId = sessionStorage.getItem("magnolia.client.v1");
    const projectionRevision = Number(document.querySelector('[data-testid="projection-revision"]')?.textContent ?? "0");
    const transcriptAfter = Number(sessionStorage.getItem("magnolia.transcript.v1") ?? "0");
    if (!sessionId || !clientId) {
      throw new Error("connected browser session was not persisted");
    }
    return await new Promise<any>((resolve, reject) => {
      const socket = new WebSocket(origin.replace("http://", "ws://") + "/api/control");
      const timer = window.setTimeout(() => reject(new Error("unsupported protocol response timed out")), 5_000);
      socket.onopen = () => socket.send(JSON.stringify({
        kind: "authenticate",
        credential: { kind: "session_id", value: sessionId },
        connect: {
          client_id: clientId,
          supported_versions: [{ major: 99, minimum_minor: 0, maximum_minor: 0 }],
        },
        cursor: {
          runtime_epoch: runtimeEpoch,
          projection_revision: projectionRevision,
          transcript_after: transcriptAfter,
        },
      }));
      socket.onmessage = (event) => {
        window.clearTimeout(timer);
        const response = JSON.parse(event.data as string);
        socket.close();
        resolve(response);
      };
      socket.onerror = () => {
        window.clearTimeout(timer);
        reject(new Error("unsupported protocol socket was rejected before negotiation"));
      };
    });
  }, { origin: host.origin, runtimeEpoch: host.runtimeEpoch });
}
