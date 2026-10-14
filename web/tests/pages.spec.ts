import { test, expect, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";

// 実物のブリッジを使い、Tauri ホストの IPC とイベント配送だけを置き換える。
test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    const host = window as any;
    const callbacks = new Map<number, Function>();
    const listeners = new Map<number, { event: string; handler: Function }>();
    let id = 0;
    let status = { panel: 0, runtime: "novr", attached: null, locked: false, trackers: [], menu: false, grabbed: false, resolution: [800, 600], size: [0.8, 0.6] };
    const dispatch = (event: string, payload: unknown) => {
      for (const listener of listeners.values()) {
        if (listener.event === event) listener.handler({ payload });
      }
    };
    const listen = async (event: string, handler: Function) => {
      const key = ++id;
      listeners.set(key, { event, handler });
      return () => listeners.delete(key);
    };
    host.testHost = { dispatch, listeners, commands: [], spawned: [], reply: null };
    host.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "panel-0" }, currentWebview: { label: "panel-0" } },
      transformCallback: (callback: Function) => { callbacks.set(++id, callback); return id; },
      invoke: async (cmd: string, args: any) => {
        if (cmd === "plugin:event|listen") {
          listeners.set(args.handler, { event: args.event, handler: callbacks.get(args.handler)! });
          return args.handler;
        }
        if (cmd === "plugin:event|unlisten") { listeners.delete(args.eventId); return; }
        if (cmd === "plugin:event|emit") { host.testHost.reply = args.payload; return; }
        if (cmd === "panel_status") return status;
        if (cmd === "pointer_ack") return;
        if (cmd === "spawn_panel") { host.testHost.spawned.push(args.page); return; }
        if (cmd === "panel_command") {
          const c = args.command;
          host.testHost.commands.push(c);
          if (c === "detach") status = { ...status, attached: null };
          if (c.attach) status = { ...status, attached: c.attach };
          if ("lock" in Object(c)) status = { ...status, locked: c.lock };
          dispatch("nyaw:status", status);
          return;
        }
        throw new Error(`予期しない IPC: ${cmd}`);
      },
    };
    host.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (_event: string, key: number) => callbacks.delete(key) };
    host.__TAURI__ = { webviewWindow: { getCurrentWebviewWindow: () => ({ listen }) } };
  });
  await page.addInitScript({ content: readFileSync(new URL("../../crates/webui/src/bridge.js", import.meta.url), "utf8") });
});

async function probe(page: Page) {
  return page.evaluate(() => {
    const host = (window as any).testHost;
    host.dispatch("webtesting:probe", 1);
    return host.reply;
  });
}

async function pointer(page: Page, type: string, target: string) {
  const box = await page.locator(`#${target}`).boundingBox();
  expect(box).not.toBeNull();
  await page.evaluate(({ type, box }) => (window as any).testHost.dispatch("nyaw:pointer", [
    { hand: "right", type, x: box!.x + box!.width / 2, y: box!.y + box!.height / 2, dx: 0, dy: -2 },
  ]), { type, box });
}

async function scrollPosition(page: Page, id: string) {
  return page.locator(`#${id}`).evaluate((el) => [el.scrollLeft, el.scrollTop]);
}

for (const id of ["scroll-list", "scroll-grid"]) {
  const both = id === "scroll-grid";
  test(`${both ? "二" : "一"}次元領域をマウス・タッチ・レーザーでスワイプできる`, async ({ page }) => {
    await page.goto("/webtesting");
    await expect(page.locator("#log")).toContainText("novr panel 0");
    const box = (await page.locator(`#${id}`).boundingBox())!;
    const start = { x: box.x + box.width - 30, y: box.y + box.height - 30 };
    const end = { x: start.x - 70, y: start.y - 45 };
    await page.mouse.move(start.x, start.y);
    await page.mouse.down();
    await page.mouse.move(end.x, end.y, { steps: 6 });
    await page.mouse.up();
    await expect.poll(() => scrollPosition(page, id)).toEqual([both ? 70 : 0, 45]);

    // タッチの信頼された PointerEvent でも同じ操作を通す。
    const cdp = await page.context().newCDPSession(page);
    await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ ...start, id: 0 }] });
    await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ ...end, id: 0 }] });
    await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    await expect.poll(() => scrollPosition(page, id)).toEqual([both ? 140 : 0, 90]);
    expect((await probe(page)).state.lastPointer.pointerType).toBe("touch");
    await cdp.detach();

    const send = async (type: string, at: typeof start) => page.evaluate(({ type, at }) => {
      (window as any).testHost.dispatch("nyaw:pointer", [{ hand: "right", type, ...at, dx: 0, dy: 0 }]);
    }, { type, at });
    await page.mouse.move(2, 2);
    await send("down", start);
    expect((await probe(page)).state.lastPointer.trusted).toBe(false);
    await send("move", end);
    await send("up", end);
    await expect.poll(() => scrollPosition(page, id)).toEqual([both ? 210 : 0, 135]);
    // 離した後に動かしてもスクロールしない。
    await send("move", start);
    expect(await scrollPosition(page, id)).toEqual([both ? 210 : 0, 135]);
    await send("down", start);
    await send("leave", start);
    await send("move", end);
    expect(await scrollPosition(page, id)).toEqual([both ? 210 : 0, 135]);
  });

  test(`${both ? "二" : "一"}次元領域をホイール・スティックで往復スクロールできる`, async ({ page }) => {
    await page.goto("/webtesting");
    await expect(page.locator("#log")).toContainText("novr panel 0");
    await page.locator(`#${id}`).hover();
    await page.mouse.wheel(80, 80);
    await expect.poll(() => scrollPosition(page, id)).toEqual([both ? 80 : 0, 80]);
    const box = (await page.locator(`#${id}`).boundingBox())!;
    const stick = (dx: number, dy: number) => page.evaluate(({ box, dx, dy }) => {
      (window as any).testHost.dispatch("nyaw:pointer", [{
        hand: "right", type: "scroll", x: box.x + 20, y: box.y + 20, dx, dy,
      }]);
    }, { box, dx, dy });
    // 右／下に倒すと scrollLeft／scrollTop が増え、左／上で戻る。
    await stick(1, -1);
    await expect.poll(() => scrollPosition(page, id)).toEqual([both ? 120 : 0, 120]);
    await stick(-1, 1);
    await expect.poll(() => scrollPosition(page, id)).toEqual([both ? 80 : 0, 80]);
    await stick(0, 0);
    expect(await scrollPosition(page, id)).toEqual([both ? 80 : 0, 80]);
  });
}

test("React の画面をマウスで操作できる", async ({ page }) => {
  await page.goto("/webtesting");
  await expect(page.locator("#log")).toContainText("novr panel 0");
  await page.locator("#click-me").click();
  await expect(page.locator("#click-me")).toHaveText("Clicked 1");
  await page.locator("#toggle").click();
  await expect(page.locator("#toggle")).toHaveClass("btn on");
  await page.locator("#text-field").fill("React");
  await expect(page.locator("#text-field")).toHaveValue("React");
  const chip = await page.locator("#drag-chip").boundingBox();
  const zone = await page.locator("#drop-zone").boundingBox();
  await page.mouse.move(chip!.x + 10, chip!.y + 10);
  await page.mouse.down();
  await page.mouse.move(zone!.x + 30, zone!.y + 30, { steps: 8 });
  await page.mouse.up();
  await expect(page.locator("#drop-zone")).toHaveClass("dropped");
  await page.locator("#scroll-list").hover();
  await page.mouse.wheel(0, 100);
  await expect.poll(async () => (await probe(page)).state.scrollTop).toBeGreaterThan(0);
  expect((await probe(page)).state.counts.pointerdown).toBeGreaterThan(0);
});

test("合成ポインタでも React のクリック・ドラッグ・スクロールが動く", async ({ page }) => {
  await page.goto("/webtesting");
  await expect(page.locator("#log")).toContainText("novr panel 0");
  await pointer(page, "move", "click-me");
  await expect.poll(async () => (await probe(page)).state.hovered).toBe(true);
  await pointer(page, "down", "click-me");
  await pointer(page, "up", "click-me");
  await expect(page.locator("#click-me")).toHaveText("Clicked 1");
  let state = (await probe(page)).state;
  expect(state.lastPointer.trusted).toBe(false);
  expect(state.counts.click).toBe(1);
  await pointer(page, "down", "drag-chip");
  await expect.poll(async () => (await probe(page)).state.captured).toBe(true);
  await pointer(page, "move", "drop-zone");
  await pointer(page, "up", "drop-zone");
  await expect(page.locator("#drop-zone")).toHaveClass("dropped");
  expect((await probe(page)).state.captured).toBe(false);
  await pointer(page, "scroll", "scroll-list");
  await expect.poll(async () => (await probe(page)).state.scrollTop).toBeGreaterThan(0);
  await pointer(page, "down", "text-field");
  await pointer(page, "up", "text-field");
  await expect(page.locator("#text-field")).toBeFocused();
  // パネルを離れたときはキャプチャもドラッグの見た目も解除される。
  await pointer(page, "down", "drag-chip");
  await pointer(page, "move", "drop-zone");
  await pointer(page, "leave", "drop-zone");
  await expect(page.locator("#drag-chip")).not.toHaveAttribute("style", /translate/);
  expect((await probe(page)).state.captured).toBe(false);
});

test("カーネル操作・状態通知・リサイズ・パネル追加を維持する", async ({ page }) => {
  await page.goto("/webtesting");
  await expect(page.locator("#log")).toContainText("novr panel 0");
  await page.locator("#lock").click();
  await expect(page.locator("#lock")).toHaveClass("btn on");
  await page.locator("#attach-left").click();
  await expect(page.locator("#attach-left")).toHaveClass("btn on");
  await page.locator("#detach").click();
  await expect(page.locator("#attach-left")).toHaveClass("btn");
  await page.locator("#resize-large").click();
  await page.setViewportSize({ width: 1000, height: 700 });
  await expect(page.locator("#size")).toContainText("viewport 1000×700");
  await page.locator("#spawn-panel").click();
  expect(await page.evaluate(() => (window as any).testHost.spawned)).toEqual(["webtesting"]);
  expect(await page.evaluate(() => (window as any).testHost.commands)).toContainEqual({ resize: { width: 1000, height: 700 } });
});

test("各 TSX ルートが直接開け、ビームの背景は透明", async ({ page }) => {
  for (const name of ["laser", "cursor"]) {
    await page.goto(`/${name}`);
    await expect(page.locator(`.${name} > div`)).toBeVisible();
    await expect(page.locator("body")).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
    await expect(page.locator("html")).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
    await page.reload();
    await expect(page.locator(`.${name} > div`)).toBeVisible();
  }
  await page.goto("/missing");
  await expect(page.getByText("ページが見つかりません")).toBeVisible();
});

test("画面を離れると購読を解除し、戻っても入力は一度だけ処理する", async ({ page }) => {
  await page.goto("/webtesting");
  await expect(page.locator("#log")).toContainText("novr panel 0");
  const subscribers = () => page.evaluate(() => [...(window as any).testHost.listeners.values()].filter((l: any) => l.event === "webtesting:probe").length);
  expect(await subscribers()).toBe(1);
  await page.evaluate(() => history.pushState(null, "", "/laser"));
  await expect(page.locator(".laser > div")).toBeVisible();
  await expect.poll(subscribers).toBe(0);
  await page.goBack();
  await expect(page.locator("#click-me")).toBeVisible();
  await expect.poll(subscribers).toBe(1);
  await page.locator("#click-me").click();
  await expect(page.locator("#click-me")).toHaveText("Clicked 1");
  expect((await probe(page)).state.counts.click).toBe(1);
});
