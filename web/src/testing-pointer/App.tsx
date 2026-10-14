// マウスはドラッグ、タッチはスワイプ。touch-action: none のチップは両方式ともドラッグする。
import { Button } from "@base-ui/react/button";
import { ScrollArea } from "@base-ui/react/scroll-area";
import { Link } from "@tanstack/react-router";
import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { nyaw, useStatus } from "../nyaw";
import { useProbe } from "../probe";

const btn =
  "rounded bg-btn px-3 py-1.5 text-sm text-ink hover:bg-btn-hover active:bg-btn-press data-[on]:bg-accent";
const TYPES = ["pointerover", "pointerdown", "pointermove", "pointerup", "pointercancel", "click"];
type Last = { type: string; target: string; pointerType: string | null } | null;

export function App() {
  const status = useStatus();
  const [taps, setTaps] = useState(0);
  const [dropped, setDropped] = useState(0);
  const [offset, setOffset] = useState<[number, number] | null>(null);
  const drag = useRef<{ id: number; x: number; y: number } | null>(null);
  const zone = useRef<HTMLDivElement>(null);
  const counts = useRef(Object.fromEntries(TYPES.map((type) => [type, 0])));
  const last = useRef<{ any: Last; pointer: Last }>({ any: null, pointer: null });
  const scrolled = useRef({ top: 0, left: 0, both: [0, 0] });

  useEffect(() => {
    const record = (event: Event) => {
      const e = event as PointerEvent;
      counts.current[e.type]++;
      if (e.type === "pointermove") return;
      const target = e.target as Element;
      const entry = {
        type: e.type,
        target: target.id || target.tagName.toLowerCase(),
        pointerType: e.pointerType ?? null,
      };
      last.current.any = entry;
      if (e.type.startsWith("pointer") && e.type !== "pointerover") last.current.pointer = entry;
    };
    for (const type of TYPES) document.addEventListener(type, record, true);
    return () => {
      for (const type of TYPES) document.removeEventListener(type, record, true);
    };
  }, []);
  useProbe(() => ({
    page: "pointer",
    taps,
    dropped,
    scrollTop: scrolled.current.top,
    scrollLeft: scrolled.current.left,
    scroll2d: scrolled.current.both,
    counts: counts.current,
    last: last.current.any,
    lastPointer: last.current.pointer,
    status,
  }));

  const finish = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (drag.current?.id !== e.pointerId) return;
    drag.current = null;
    setOffset(null);
  };
  return (
    <main className="flex h-full flex-col gap-2.5 overflow-hidden bg-panel p-3 text-ink">
      <div className="flex flex-wrap items-center gap-2">
        <Link id="back" to="/testing/index.html" className={btn}>
          Back
        </Link>
        <Button
          id="pointer-mode"
          className={btn}
          data-on={status?.pointer_mode === "touch" ? "" : undefined}
          onClick={() =>
            void nyaw().command({
              pointer_mode: status?.pointer_mode === "touch" ? "mouse" : "touch",
            })
          }
        >
          Mode: {status ? (status.pointer_mode === "touch" ? "Touch" : "Mouse") : "-"}
        </Button>
        <Button id="tap-me" className={btn} onClick={() => setTaps((n) => n + 1)}>
          Tapped {taps}
        </Button>
      </div>
      <div className="flex items-center gap-2">
        <div
          id="drag-chip"
          className="h-8 w-16 touch-none rounded-full bg-accent"
          style={offset ? { transform: `translate(${offset[0]}px, ${offset[1]}px)` } : undefined}
          onPointerDown={(e) => {
            drag.current = { id: e.pointerId, x: e.clientX, y: e.clientY };
            e.currentTarget.setPointerCapture(e.pointerId);
            e.preventDefault();
          }}
          onPointerMove={(e) => {
            if (drag.current?.id === e.pointerId)
              setOffset([e.clientX - drag.current.x, e.clientY - drag.current.y]);
          }}
          onPointerUp={(e) => {
            if (drag.current?.id !== e.pointerId) return;
            const r = zone.current!.getBoundingClientRect();
            if (
              e.clientX >= r.left &&
              e.clientX <= r.right &&
              e.clientY >= r.top &&
              e.clientY <= r.bottom
            )
              setDropped((n) => n + 1);
            finish(e);
          }}
          onPointerCancel={finish}
          onLostPointerCapture={finish}
        />
        <div
          ref={zone}
          id="drop-zone"
          className={`grid h-14 w-36 place-items-center rounded-md border-2 border-dashed border-btn-hover text-sm ${dropped ? "bg-ok" : ""}`}
        >
          Drop
        </div>
      </div>
      <p id="log" className="font-mono text-xs">
        mode {status?.pointer_mode ?? "-"} / taps {taps} / dropped {dropped}
      </p>
      <div className="grid min-h-0 flex-1 grid-cols-2 gap-3">
        {(["y", "both"] as const).map((axis) => (
          <section key={axis} className="flex min-h-0 min-w-0 flex-col gap-2">
            <h2 className="text-xs">
              {axis === "y" ? "一次元" : "二次元"}（スワイプ／スティック）
            </h2>
            <ScrollArea.Root className="min-h-0 flex-1 bg-black/30">
              <ScrollArea.Viewport
                id={axis === "y" ? "swipe-list" : "swipe-grid"}
                className="h-full touch-auto"
                role="region"
                aria-label={axis === "y" ? "一次元スクロール" : "二次元スクロール"}
                onScroll={(e) => {
                  const { scrollLeft, scrollTop } = e.currentTarget;
                  if (axis === "y") {
                    scrolled.current.top = scrollTop;
                    scrolled.current.left = scrollLeft;
                  } else scrolled.current.both = [scrollLeft, scrollTop];
                }}
              >
                {axis === "y" ? (
                  Array.from({ length: 40 }, (_, i) => (
                    <div key={i} className="h-10 border-b border-white/10 p-2 text-sm">
                      Swipe row {i}
                    </div>
                  ))
                ) : (
                  <div className="grid w-[1200px] grid-cols-12 auto-rows-[60px]">
                    {Array.from({ length: 240 }, (_, i) => (
                      <div key={i} className="border border-white/10 p-2 text-sm">
                        {i % 12}, {Math.floor(i / 12)}
                      </div>
                    ))}
                  </div>
                )}
              </ScrollArea.Viewport>
              <ScrollArea.Scrollbar className="w-1.5">
                <ScrollArea.Thumb className="w-full rounded bg-btn-hover" />
              </ScrollArea.Scrollbar>
              {axis === "both" && (
                <ScrollArea.Scrollbar orientation="horizontal" className="h-1.5">
                  <ScrollArea.Thumb className="h-full rounded bg-btn-hover" />
                </ScrollArea.Scrollbar>
              )}
              <ScrollArea.Corner />
            </ScrollArea.Root>
          </section>
        ))}
      </div>
    </main>
  );
}
