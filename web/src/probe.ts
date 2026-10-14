// E2E 用の問い合わせ口。Rust のテストが Tauri のイベント `testing:probe`（中身は番号）をこのパネルへ送ると、
// ページの状態と id を持つ要素の位置を `testing:state` で返す。GPUI の `debug_selector` に当たる。
import { emit } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useEffect, useRef } from "react";

export const PROBE_EVENT = "testing:probe";
export const STATE_EVENT = "testing:state";

const rects = () =>
  Object.fromEntries(
    [...document.querySelectorAll("[id]")].map((el) => {
      const r = el.getBoundingClientRect();
      return [el.id, { x: r.x, y: r.y, w: r.width, h: r.height }];
    }),
  );

/** id を持つ要素のうち、CSS 用の属性が付いているもの。 */
const marked = (attr: string) => [...document.querySelectorAll(`[id][${attr}]`)].map((el) => el.id);

export function useProbe(state: () => unknown) {
  const latest = useRef(state);
  useEffect(() => {
    latest.current = state;
  });
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const off = getCurrentWebviewWindow().listen<number>(PROBE_EVENT, (e) => {
      void emit(STATE_EVENT, {
        seq: e.payload,
        state: latest.current(),
        rects: rects(),
        focused: document.activeElement?.id || null,
        hover: marked("data-nyaw-hover"),
        active: marked("data-nyaw-active"),
        viewport: [innerWidth, innerHeight],
      });
    });
    return () => {
      void off.then((f) => f());
    };
  }, []);
}
