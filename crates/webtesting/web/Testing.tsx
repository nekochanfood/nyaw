import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { ScrollArea } from "../../webui/web/ScrollArea";
import "./testing.css";

const TYPES = [
  "pointerover", "pointerenter", "pointerdown", "pointermove", "pointerup", "pointercancel",
  "pointerout", "pointerleave", "gotpointercapture", "lostpointercapture",
  "mouseover", "mouseenter", "mousedown", "mousemove", "mouseup", "mouseout", "mouseleave",
  "click", "wheel",
] as const;

interface EventInfo {
  type: string;
  target: string;
  x: number;
  y: number;
  pointerType: string | null;
  pointerId: number | null;
  buttons: number;
  trusted: boolean;
}

function initialState() {
  return {
    clicks: 0, hovered: false, toggled: false,
    counts: Object.fromEntries(TYPES.map((type) => [type, 0])),
    last: null as EventInfo | null,
    lastPointer: null as EventInfo | null,
    captured: false, dragMoves: 0, dropped: false,
    wheel: 0, scrollTop: 0, scrollLeft: 0, scroll2d: [0, 0], focused: false, resizes: 0,
    viewport: [innerWidth, innerHeight],
    status: null as WebStatus | null,
  };
}

export function Testing() {
  const [state, setState] = useState(initialState);
  const latest = useRef(state);
  latest.current = state;
  const [error, setError] = useState("");
  const [offset, setOffset] = useState<[number, number] | null>(null);
  const [over, setOver] = useState(false);
  const drag = useRef<{ id: number; x: number; y: number } | null>(null);
  const zone = useRef<HTMLDivElement>(null);
  const clickMe = useRef<HTMLButtonElement>(null);
  const reportError = (error: unknown) => setError(String(error));
  const command: Window["nyaw"]["command"] = (cmd) => window.nyaw.command(cmd).catch(reportError);

  useEffect(() => {
    const record = (event: Event) => {
      const e = event as PointerEvent;
      const target = e.target as Element;
      const last = e.type === "pointermove" || e.type === "mousemove" ? null : {
        type: e.type, target: target.id || target.tagName.toLowerCase(),
        x: e.clientX, y: e.clientY,
        pointerType: e.pointerType ?? null, pointerId: e.pointerId ?? null,
        buttons: e.buttons, trusted: e.isTrusted,
      };
      setState((prev) => ({
        ...prev,
        counts: { ...prev.counts, [e.type]: prev.counts[e.type] + 1 },
        last: last ?? prev.last,
        lastPointer: last && e.type.startsWith("pointer") ? last : prev.lastPointer,
      }));
    };
    const resized = () => setState((prev) => ({
      ...prev, resizes: prev.resizes + 1, viewport: [innerWidth, innerHeight],
    }));
    for (const type of TYPES) document.addEventListener(type, record, true);
    window.addEventListener("resize", resized);
    return () => {
      for (const type of TYPES) document.removeEventListener(type, record, true);
      window.removeEventListener("resize", resized);
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    let retry: ReturnType<typeof setTimeout>;
    const onStatus = (status: WebStatus) => {
      if (!disposed) setState((prev) => ({ ...prev, status }));
    };
    const unlisten = window.nyaw.onStatus(onStatus);
    // 起動直後はまだカーネルの状態が無いので、取れるまで問い合わせる。
    const first = () => window.nyaw.status().then(onStatus, () => {
      if (!disposed) retry = setTimeout(first, 100);
    });
    void first();
    return () => { disposed = true; clearTimeout(retry); unlisten(); };
  }, []);

  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void getCurrentWebviewWindow().listen<number>("webtesting:probe", (e) => {
      const rects = Object.fromEntries([...document.querySelectorAll("[id]")].map((el) => {
        const r = el.getBoundingClientRect();
        return [el.id, { x: r.x, y: r.y, w: r.width, h: r.height }];
      }));
      void emit("webtesting:state", {
        seq: e.payload, state: latest.current, rects,
        focused: document.activeElement?.id ?? null,
        hoverAttr: clickMe.current?.hasAttribute("data-nyaw-hover") ?? false,
        screen: [screenX, screenY],
      }).catch(reportError);
    }).then((unlisten) => { if (disposed) unlisten(); else stop = unlisten; }, reportError);
    return () => { disposed = true; stop?.(); };
  }, []);

  const inside = (e: ReactPointerEvent) => {
    const r = zone.current!.getBoundingClientRect();
    return e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom;
  };
  const endDrag = () => { drag.current = null; setOffset(null); setOver(false); };
  const status = state.status;
  const btn = (on: boolean | undefined) => `btn${on ? " on" : ""}`;
  return (
    <main className="testing">
      <title>Testing (Web)</title>
      <section>
        <h2>VR（カーネル）</h2>
        <button className={btn(status?.attached === "hmd")} id="attach-hmd" onClick={() => command({ attach: "hmd" })}>Attach HMD</button>
        <button className={btn(status?.attached === "left")} id="attach-left" onClick={() => command({ attach: "left" })}>Attach Left</button>
        <button className={btn(status?.attached === "right")} id="attach-right" onClick={() => command({ attach: "right" })}>Attach Right</button>
        <button className="btn" id="detach" onClick={() => command("detach")}>Detach</button>
        <button className={btn(status?.locked)} id="lock" onClick={() => command({ lock: !status?.locked })}>Lock</button>
        <button className="btn" id="spawn-panel" onClick={() => window.nyaw.spawn("webtesting").catch(reportError)}>Spawn Panel</button>
        <button className="btn" id="close" onClick={() => command("close")}>Close</button>
        <span className={btn(status?.menu)} id="menu-held">Menu</span>
        <span className={btn(status?.grabbed)} id="grabbed">Grabbed</span>
        <h2>リサイズ</h2>
        <button className="btn" id="resize-small" onClick={() => command({ resize: { width: 640, height: 480 } })}>640×480</button>
        <button className="btn" id="resize-large" onClick={() => command({ resize: { width: 1000, height: 700 } })}>1000×700</button>
        <span id="size">viewport {state.viewport.join("×")}{status && ` / panel ${status.resolution.join("×")}px ${status.size.map((m) => m.toFixed(2)).join("×")}m`}</span>
        <h2>状態</h2>
        <div id="log">{[
          status ? `${status.runtime} panel ${status.panel}` : "status: -",
          `last: ${state.last ? `${state.last.type} ${state.last.target} (${state.last.x | 0},${state.last.y | 0}) ${state.last.pointerType}` : "-"}`,
          `clicks ${state.clicks}  wheel ${state.wheel}  scrollTop ${state.scrollTop | 0}`,
          error,
        ].filter(Boolean).join("\n")}</div>
        <h2>フォーカス</h2>
        <input id="text-field" placeholder="Type here"
          onFocus={() => setState((prev) => ({ ...prev, focused: true }))}
          onBlur={() => setState((prev) => ({ ...prev, focused: false }))} />
      </section>
      <section>
        <h2>ポインタ（DOM イベント）</h2>
        <button ref={clickMe} className={`btn${state.clicks ? " clicked" : ""}`} id="click-me"
          onClick={() => setState((prev) => ({ ...prev, clicks: prev.clicks + 1 }))}
          onPointerEnter={() => setState((prev) => ({ ...prev, hovered: true }))}
          onPointerLeave={() => setState((prev) => ({ ...prev, hovered: false }))}>
          {state.clicks ? `Clicked ${state.clicks}` : "Click Me"}
        </button>
        <button className={btn(state.toggled)} id="toggle" onClick={() => setState((prev) => ({ ...prev, toggled: !prev.toggled }))}>Toggle</button>
        <h2>ドラッグ（setPointerCapture）</h2>
        <div id="drag-chip" style={{ transform: offset ? `translate(${offset[0]}px, ${offset[1]}px)` : undefined }}
          onPointerDown={(e) => {
            drag.current = { id: e.pointerId, x: e.clientX, y: e.clientY };
            e.currentTarget.setPointerCapture(e.pointerId);
            e.preventDefault();
          }}
          onGotPointerCapture={() => setState((prev) => ({ ...prev, captured: true }))}
          onLostPointerCapture={() => { setState((prev) => ({ ...prev, captured: false })); endDrag(); }}
          onPointerMove={(e) => {
            const start = drag.current;
            if (!start || e.pointerId !== start.id) return;
            setState((prev) => ({ ...prev, dragMoves: prev.dragMoves + 1 }));
            setOffset([e.clientX - start.x, e.clientY - start.y]);
            setOver(inside(e));
          }}
          onPointerUp={(e) => {
            if (!drag.current || e.pointerId !== drag.current.id) return;
            if (inside(e)) setState((prev) => ({ ...prev, dropped: true }));
            endDrag();
          }}
          onPointerCancel={endDrag} />
        <div ref={zone} id="drop-zone" className={`${over ? "over " : ""}${state.dropped ? "dropped" : ""}`}>Drop</div>
        <h2>一次元スクロール（スワイプ／スティック）</h2>
        <ScrollArea id="scroll-list" axis="y" role="region" aria-label="一次元スクロール"
          onWheel={(e) => { const delta = e.deltaY; setState((prev) => ({ ...prev, wheel: prev.wheel + delta })); }}
          onScroll={(e) => { const { scrollTop, scrollLeft } = e.currentTarget; setState((prev) => ({ ...prev, scrollTop, scrollLeft })); }}>
          {Array.from({ length: 30 }, (_, i) => <div key={i}>Row {i}</div>)}
        </ScrollArea>
        <h2>二次元スクロール（スワイプ／スティック）</h2>
        <ScrollArea id="scroll-grid" axis="both" role="region" aria-label="二次元スクロール"
          onScroll={(e) => { const el = e.currentTarget; const scroll2d = [el.scrollLeft, el.scrollTop]; setState((prev) => ({ ...prev, scroll2d })); }}>
          <div className="scroll-plane">
            {Array.from({ length: 120 }, (_, i) => <div key={i}>{i % 12}, {Math.floor(i / 12)}</div>)}
          </div>
        </ScrollArea>
      </section>
    </main>
  );
}
