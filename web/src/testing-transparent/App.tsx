// 半透明の確認用ページ（GPUI の testing-transparent の Web 版）。背景は透明で、不透明度の違う要素を並べる。
// VR ではパネルの外が透けて見え、半透明の要素はその割合で重なって見えるはず。
// E2E は読み戻したテクスチャのアルファ（ストレートアルファ）で確かめる。
import { Button } from "@base-ui/react/button";
import { useState } from "react";
import { nyaw } from "../nyaw";
import { useProbe } from "../probe";

/** 各要素の色と不透明度。E2E がこの値と比べる。 */
export const SWATCHES = [
  { id: "opaque", className: "bg-[#2ecc71]", rgb: [0x2e, 0xcc, 0x71], alpha: 1 },
  { id: "half", className: "bg-black/50", rgb: [0, 0, 0], alpha: 0.5 },
  { id: "faint", className: "bg-white/25", rgb: [255, 255, 255], alpha: 0.25 },
] as const;

export function App() {
  const [clicks, setClicks] = useState(0);
  useProbe(() => ({ clicks }));
  return (
    // 背景は塗らない（body / html も透明）。
    <main className="flex h-full flex-col items-start gap-4 p-6 text-white">
      {SWATCHES.map((s) => (
        <div key={s.id} id={s.id} className={`h-16 w-64 rounded-lg ${s.className}`} />
      ))}
      <div className="flex gap-2">
        <Button
          id="glass-button"
          className="rounded bg-white/40 px-3 py-1.5 text-sm hover:bg-white/60"
          onClick={() => setClicks((n) => n + 1)}
        >
          {clicks ? `Clicked ${clicks}` : "Click (半透明)"}
        </Button>
        <Button
          id="close"
          className="rounded bg-black/60 px-3 py-1.5 text-sm hover:bg-black/80"
          onClick={() => void nyaw().command("close")}
        >
          Close
        </Button>
      </div>
    </main>
  );
}
