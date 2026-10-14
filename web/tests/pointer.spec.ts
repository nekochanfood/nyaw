import { test, expect, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";

// 入力ブリッジは実物を使い、Tauri ホストの IPC とイベント配送だけを置き換える。
test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    const host = window as any;
    const callbacks = new Map<number, (event: { payload: unknown }) => void>();
    const listeners = new Map<
      number,
      { event: string; handler: (event: { payload: unknown }) => void }
    >();
    let id = 0;
    let status = {
      panel: 0,
      runtime: "novr",
      pointer_mode: "mouse",
      attached: null,
      locked: false,
      trackers: [],
      menu: false,
      grabbed: false,
      resolution: [800, 600],
      size: [0.8, 0.6],
    };
    const dispatch = (event: string, payload: unknown) => {
      for (const listener of listeners.values())
        if (listener.event === event) listener.handler({ payload });
    };
    const listen = async (event: string, handler: (event: { payload: unknown }) => void) => {
      const key = ++id;
      listeners.set(key, { event, handler });
      return () => listeners.delete(key);
    };
    host.testHost = { dispatch, listeners, reply: null };
    host.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "panel-0" }, currentWebview: { label: "panel-0" } },
      transformCallback: (callback: (event: { payload: unknown }) => void) => {
        callbacks.set(++id, callback);
        return id;
      },
      invoke: async (cmd: string, args: any) => {
        if (cmd === "plugin:event|listen") {
          listeners.set(args.handler, { event: args.event, handler: callbacks.get(args.handler)! });
          return args.handler;
        }
        if (cmd === "plugin:event|unlisten") {
          listeners.delete(args.eventId);
          return;
        }
        if (cmd === "plugin:event|emit") {
          host.testHost.reply = args.payload;
          return;
        }
        if (cmd === "panel_status") return status;
        if (cmd === "pointer_ack") return;
        if (cmd === "panel_command") {
          if (args.command.pointer_mode)
            status = { ...status, pointer_mode: args.command.pointer_mode };
          dispatch("nyaw:status", status);
          return;
        }
        throw new Error(`予期しない IPC: ${cmd}`);
      },
    };
    host.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener: (_event: string, key: number) => callbacks.delete(key),
    };
    host.__TAURI__ = { webviewWindow: { getCurrentWebviewWindow: () => ({ listen }) } };
  });
  await page.addInitScript({
    content: readFileSync(new URL("../../crates/ui/src/web/bridge.js", import.meta.url), "utf8"),
  });
});

const probe = (page: Page) =>
  page.evaluate(() => {
    const host = (window as any).testHost;
    host.dispatch("testing:probe", 1);
    return host.reply;
  });
const position = (page: Page, id: string) =>
  page.locator(`#${id}`).evaluate((el) => [el.scrollLeft, el.scrollTop]);
const send = (
  page: Page,
  type: string,
  at: { x: number; y: number },
  mode: string,
  dx = 0,
  dy = 0,
) =>
  page.evaluate(
    (p) => {
      (window as any).testHost.dispatch("nyaw:pointer", [p]);
    },
    { hand: "right", type, ...at, mode, dx, dy },
  );
const center = async (page: Page, id: string) => {
  const box = (await page.locator(`#${id}`).boundingBox())!;
  return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
};
const openPointer = async (page: Page) => {
  await page.goto("/testing/index.html");
  await page.locator("#open-pointer").click();
  await expect(page.locator("#pointer-mode")).toHaveText("Mode: Mouse");
};

for (const [id, both] of [
  ["swipe-list", false],
  ["swipe-grid", true],
] as const) {
  test(`${both ? "二" : "一"}次元：マウスはドラッグ、タッチはスワイプ`, async ({ page }) => {
    await openPointer(page);
    const start = await center(page, id);
    const end = { x: start.x - 60, y: start.y - 60 };
    await send(page, "down", start, "mouse");
    await send(page, "move", end, "mouse");
    await send(page, "up", end, "mouse");
    expect(await position(page, id)).toEqual([0, 0]);
    expect((await probe(page)).state.counts.pointercancel).toBe(0);
    await page.locator("#pointer-mode").click();
    await expect(page.locator("#pointer-mode")).toHaveText("Mode: Touch");
    await page.mouse.move(2, 2);
    await send(page, "down", start, "touch");
    await send(page, "move", end, "touch");
    await send(page, "up", end, "touch");
    await expect.poll(() => position(page, id)).toEqual([both ? 60 : 0, 60]);
    const reply = await probe(page);
    expect(reply.state.counts.pointercancel).toBe(1);
    expect(reply.state.lastPointer.pointerType).toBe("touch");
    expect(reply.state.taps).toBe(0);
    await send(page, "move", start, "touch");
    expect(await position(page, id)).toEqual([both ? 60 : 0, 60]);
  });

  for (const mode of ["mouse", "touch"]) {
    test(`${both ? "二" : "一"}次元：${mode} 方式でもスティックの両軸と逆方向が効く`, async ({
      page,
    }) => {
      await openPointer(page);
      if (mode === "touch") await page.locator("#pointer-mode").click();
      const at = await center(page, id);
      await send(page, "scroll", at, mode, 1, -1);
      await expect.poll(() => position(page, id)).toEqual([both ? 40 : 0, 40]);
      await send(page, "scroll", at, mode, -1, 1);
      await expect.poll(() => position(page, id)).toEqual([0, 0]);
      await send(page, "scroll", at, mode, 0, 0);
      expect(await position(page, id)).toEqual([0, 0]);
    });
  }
}

for (const mode of ["mouse", "touch"]) {
  test(`${mode} 方式でタップと touch-action:none のドラッグが動く`, async ({ page }) => {
    await openPointer(page);
    if (mode === "touch") await page.locator("#pointer-mode").click();
    await page.mouse.move(2, 2);
    const tap = await center(page, "tap-me");
    await send(page, "down", tap, mode);
    await send(page, "up", tap, mode);
    await expect(page.locator("#tap-me")).toHaveText("Tapped 1");
    const chip = await center(page, "drag-chip");
    const zone = await center(page, "drop-zone");
    await send(page, "down", chip, mode);
    await send(page, "move", zone, mode);
    await send(page, "up", zone, mode);
    await expect.poll(async () => (await probe(page)).state.dropped).toBe(1);
    expect((await probe(page)).state.counts.pointercancel).toBe(0);
    await send(page, "down", chip, mode);
    await send(page, "move", zone, mode);
    await send(page, "leave", zone, mode);
    await expect(page.locator("#drag-chip")).not.toHaveAttribute("style", /translate/);
    await page.locator("#back").click();
    const testingChip = await center(page, "drag-chip");
    const testingZone = await center(page, "drop-zone");
    await send(page, "down", testingChip, mode);
    await send(page, "move", testingZone, mode);
    await expect(page.locator("#drag-chip")).toHaveAttribute("style", /translate/);
    await send(page, "leave", testingZone, mode);
    await expect(page.locator("#drag-chip")).not.toHaveAttribute("style", /translate/);
  });
}

test("ルートの直接表示と再読み込み、戻る操作で画面が切り替わる", async ({ page }) => {
  for (const route of ["testing", "testing-pointer", "testing-transparent"]) {
    await page.goto(`/${route}/index.html`);
    await expect(page.locator("main")).toBeVisible();
    await page.reload();
    await expect(page.locator("main")).toBeVisible();
  }
  await expect(page.locator("body")).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
  await openPointer(page);
  await page.locator("#back").click();
  await expect(page.locator("#open-pointer")).toBeVisible();
  const listeners = await page.evaluate(
    () =>
      [...(window as any).testHost.listeners.values()].filter(
        (l: any) => l.event === "testing:probe",
      ).length,
  );
  expect(listeners).toBe(1);
});
