import { animate } from "animejs";
import { useEffect } from "react";

import { reducedMotion } from "@/motion/reducedMotion";

/*
  ボタンを押した位置から円が広がる波紋。ルートで useRipple() を 1 回呼べば、document への委譲で全ボタンに付く。
  - <button>、role="button"、メニューの項目（role="menuitem*"）、タブ（role="tab"）は自動で対象になる
  - それ以外（リンクなど）に付けるときは data-ripple を付ける。外すときは要素か祖先に data-no-ripple を付ける
  波紋はホストの末尾に差し込んだコンテナの中に描き、角の形に合わせて切り取る。色は文字色（currentColor）。
  動きは anime.js で付ける（WAAPI を使うと、Base UI がポップアップを閉じるときに波紋の終わりまで待ってしまう）。
*/

const HOST_SELECTOR = "button, [role='button'], [role^='menuitem'], [role='tab'], [data-ripple]";

/** 波紋の不透明度。文字色に掛かる */
const OPACITY = 0.2;
/** 押した点から全面を覆うまで広がる時間（ms） */
const EXPAND_MS = 450;
/** すぐに離しても、最低これだけ広がってから消え始める（ms） */
const MIN_EXPAND_MS = 150;
/** 離してから消えるまでの時間（ms） */
const FADE_MS = 300;

/** 押した点 (x, y) から、ホストの矩形の最も遠い角までの距離。これを半径にすれば必ず全面を覆う */
export function rippleRadius(width: number, height: number, x: number, y: number) {
  return Math.hypot(Math.max(x, width - x), Math.max(y, height - y));
}

/** 押してから離すまでの時間（ms）から、消え始めるまでの待ち時間（ms）を返す */
export function fadeDelay(elapsed: number) {
  return Math.max(0, MIN_EXPAND_MS - elapsed);
}

function findHost(target: EventTarget | null): HTMLElement | null {
  if (!(target instanceof Element) || target.closest("[data-no-ripple]")) return null;
  const host = target.closest<HTMLElement>(HOST_SELECTOR);
  if (!host || (host instanceof HTMLButtonElement && host.disabled)) return null;
  return host.getAttribute("aria-disabled") === "true" ? null : host;
}

/** ホストの直下の波紋のコンテナ。なければ作る */
function container(host: HTMLElement) {
  const found = host.querySelector<HTMLElement>(":scope > [data-ripple-container]");
  if (found) return found;
  // コンテナの位置の基準にする。absolute などのボタンはそのまま使う
  if (getComputedStyle(host).position === "static") host.style.position = "relative";
  const el = document.createElement("span");
  el.dataset.rippleContainer = "";
  el.setAttribute("aria-hidden", "true");
  // 角の形も受け継ぎ、角丸の外へ波紋が出ないようにする
  el.className =
    "pointer-events-none absolute inset-0 overflow-hidden rounded-[inherit] [corner-shape:inherit]";
  host.appendChild(el);
  return el;
}

/** ホストの中の (x, y)（ビューポートの座標）から波紋を広げる。戻り値を呼ぶと消え、終わると DOM から外れる */
export function spawnRipple(host: HTMLElement, x: number, y: number) {
  const box = container(host);
  const rect = box.getBoundingClientRect();
  const [width, height] = [box.offsetWidth, box.offsetHeight];
  // ホストが拡大・縮小されていても、ホストの中の座標に直して置く
  const localX = (x - rect.left) / (width > 0 ? rect.width / width : 1);
  const localY = (y - rect.top) / (height > 0 ? rect.height / height : 1);
  const radius = rippleRadius(width, height, localX, localY);

  const circle = document.createElement("span");
  circle.className = "pointer-events-none absolute rounded-full bg-current";
  Object.assign(circle.style, {
    width: `${radius * 2}px`,
    height: `${radius * 2}px`,
    left: `${localX - radius}px`,
    top: `${localY - radius}px`,
    opacity: `${OPACITY}`,
  });
  box.appendChild(circle);

  const startedAt = performance.now();
  animate(circle, { scale: [0, 1], duration: EXPAND_MS, ease: "out(4)" });
  let released = false;
  return () => {
    if (released) return;
    released = true;
    animate(circle, {
      opacity: 0,
      duration: FADE_MS,
      delay: fadeDelay(performance.now() - startedAt),
      ease: "out(2)",
      onComplete: () => circle.remove(),
    });
  };
}

/** 全ボタンに波紋を付ける。ルートから 1 回だけ呼ぶ */
export function useRipple() {
  useEffect(() => {
    // 押している間だけ広げておき、離したら消す。マルチタッチに備えて pointerId ごとに持つ
    const active = new Map<number, () => void>();
    const release = (id: number) => {
      active.get(id)?.();
      active.delete(id);
    };
    const down = (e: PointerEvent) => {
      // 左クリックとタッチ・ペンだけ
      if (e.button !== 0 || reducedMotion()) return;
      const host = findHost(e.target);
      if (!host) return;
      release(e.pointerId);
      active.set(e.pointerId, spawnRipple(host, e.clientX, e.clientY));
    };
    // タッチでスクロールが始まると pointercancel が来るので、それでも消す
    const end = (e: PointerEvent) => release(e.pointerId);
    // キーボード（Enter・Space）で押したときは位置がないので、中央から出す。ポインタの click は detail が 1 以上
    const click = (e: MouseEvent) => {
      if (e.detail !== 0 || reducedMotion()) return;
      const host = findHost(e.target);
      if (!host) return;
      const rect = host.getBoundingClientRect();
      spawnRipple(host, rect.left + rect.width / 2, rect.top + rect.height / 2)();
    };
    // capture で拾い、ボタン側の stopPropagation に邪魔されないようにする
    document.addEventListener("pointerdown", down, true);
    window.addEventListener("pointerup", end, true);
    window.addEventListener("pointercancel", end, true);
    document.addEventListener("click", click, true);
    return () => {
      document.removeEventListener("pointerdown", down, true);
      window.removeEventListener("pointerup", end, true);
      window.removeEventListener("pointercancel", end, true);
      document.removeEventListener("click", click, true);
      for (const fade of active.values()) fade();
      active.clear();
    };
  }, []);
}
