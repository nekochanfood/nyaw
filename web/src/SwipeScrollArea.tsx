import { ScrollArea } from "@base-ui/react/scroll-area";
import { useRef, type ComponentProps, type PointerEvent } from "react";

// 合成ポインタにもスワイプを用意する。ホイールとスティックはブラウザ／ブリッジが処理する。
export function SwipeScrollArea({ axis, className = "", ...props }: ComponentProps<typeof ScrollArea.Viewport> & { axis: "y" | "both" }) {
  const drag = useRef<{ id: number; x: number; y: number; left: number; top: number } | null>(null);
  const finish = (e: PointerEvent<HTMLDivElement>) => {
    if (drag.current?.id !== e.pointerId) return;
    drag.current = null;
    if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId);
  };
  return <ScrollArea.Root className="h-20 w-full min-w-0 bg-black/30">
    <ScrollArea.Viewport {...props} className={`h-full touch-none outline-none focus-visible:outline-2 focus-visible:outline-accent ${className}`}
      style={{ overflowX: axis === "y" ? "hidden" : "auto", overflowY: "auto" }}
      onPointerDown={(e) => {
        if (e.button !== 0 || drag.current) return;
        const el = e.currentTarget;
        drag.current = { id: e.pointerId, x: e.clientX, y: e.clientY, left: el.scrollLeft, top: el.scrollTop };
        el.setPointerCapture(e.pointerId);
        el.focus({ preventScroll: true });
        e.preventDefault();
      }}
      onPointerMove={(e) => {
        const start = drag.current;
        if (!start || start.id !== e.pointerId) return;
        if (axis === "both") e.currentTarget.scrollLeft = start.left + start.x - e.clientX;
        e.currentTarget.scrollTop = start.top + start.y - e.clientY;
      }}
      onPointerUp={finish} onPointerCancel={finish} onLostPointerCapture={finish} />
    <ScrollArea.Scrollbar className="w-1.5"><ScrollArea.Thumb className="w-full rounded bg-btn-hover" /></ScrollArea.Scrollbar>
    {axis === "both" && <ScrollArea.Scrollbar orientation="horizontal" className="h-1.5"><ScrollArea.Thumb className="h-full rounded bg-btn-hover" /></ScrollArea.Scrollbar>}
    <ScrollArea.Corner />
  </ScrollArea.Root>;
}
