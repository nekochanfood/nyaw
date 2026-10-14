import { useRef, type ComponentProps, type PointerEvent } from "react";
import "./scroll.css";

// レーザーの合成イベントにもスワイプの既定動作を用意する。ホイールはブラウザ／ブリッジが処理する。
export function ScrollArea({ axis, className = "", ...props }: ComponentProps<"div"> & { axis: "y" | "both" }) {
  const drag = useRef<{ id: number; x: number; y: number; left: number; top: number } | null>(null);
  const finish = (e: PointerEvent<HTMLDivElement>) => {
    if (drag.current?.id !== e.pointerId) return;
    drag.current = null;
    if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId);
  };
  return <div {...props} className={`scroll-area ${className}`} data-axis={axis} tabIndex={0}
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
      const el = e.currentTarget;
      if (axis === "both") el.scrollLeft = start.left + start.x - e.clientX;
      el.scrollTop = start.top + start.y - e.clientY;
    }}
    onPointerUp={finish} onPointerCancel={finish} onLostPointerCapture={finish} />;
}
