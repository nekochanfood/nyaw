import { animate } from "animejs";
import { useCallback, useLayoutEffect, useRef } from "react";

import { type Duration, duration } from "@/motion/duration";
import { reducedMotion } from "@/motion/reducedMotion";

/** 出入りの動き。`axis` 方向に `offset` px ずれた位置からフェードしながら戻る。時間はトークンの名前 */
export type Presence = { axis: "x" | "y"; offset: number; enter: Duration; exit: Duration };

export type Side = "top" | "bottom" | "left" | "right";

/** アンカーから離れる向きに `distance` px だけスライドする出入り */
export function slideFrom(
  side: Side,
  distance: number,
  enter: Duration,
  exit: Duration,
): Presence {
  const axis = side === "top" || side === "bottom" ? "y" : "x";
  // 下や右に出るものは、アンカー側（上や左）から現れる
  const offset = side === "bottom" || side === "right" ? -distance : distance;
  return { axis, offset, enter, exit };
}

export const DROPDOWN_PRESENCE = {
  bottom: slideFrom("bottom", 12, "normal", "quick"),
  top: slideFrom("top", 12, "normal", "quick"),
  left: slideFrom("left", 12, "normal", "quick"),
  right: slideFrom("right", 12, "normal", "quick"),
};

export const TOOLTIP_PRESENCE = {
  bottom: slideFrom("bottom", 8, "quick", "fast"),
  top: slideFrom("top", 8, "quick", "fast"),
  left: slideFrom("left", 8, "quick", "fast"),
  right: slideFrom("right", 8, "quick", "fast"),
};

/**
 * Base UI は閉じたポップアップを `getAnimations()`（WAAPI）が終わるまで残してから外す。anime.js は WAAPI を
 * 使わないので、退場の間だけ中身のない WAAPI のアニメーションを走らせて引き止め、戻り値を呼んで放す。
 */
export function holdPopup(el: Element) {
  // ponytail: 放し忘れても 10 秒で外れる
  const hold = el.animate([], { duration: 10_000 });
  return () => hold.finish();
}

/**
 * Base UI のポップアップを anime.js で出し入れする。返す ref をポップアップに付け、Root は `open` で制御する。
 * `presence` はモジュールの定数を渡す。
 */
export function usePopupPresence<T extends HTMLElement>(open: boolean, presence: Presence) {
  const { axis, offset, enter, exit } = presence;
  const elRef = useRef<T | null>(null);
  const release = useRef<(() => void) | null>(null);
  const justMounted = useRef(false);

  // ポップアップは開くたびに挿入される。入場は ref が付いた時点（描画の前）に始める
  const ref = useCallback(
    (el: T | null) => {
      elRef.current = el;
      if (!el) return;
      justMounted.current = true;
      if (reducedMotion()) return;
      animate(el, {
        opacity: [0, 1],
        [axis]: [offset, 0],
        duration: duration(enter),
        ease: "out(3)",
      });
    },
    [axis, offset, enter],
  );

  useLayoutEffect(() => {
    const el = elRef.current;
    const mounted = justMounted.current;
    justMounted.current = false;
    if (!el || (open && mounted)) return;
    const done = () => {
      release.current?.();
      release.current = null;
    };
    if (reducedMotion()) return done();
    // 退場の途中で開き直したときは、今の状態から戻す。引き止めは戻りきってから放す
    if (!open) release.current ??= holdPopup(el);
    const anim = animate(el, {
      opacity: open ? 1 : 0,
      [axis]: open ? 0 : offset,
      duration: duration(open ? enter : exit),
      ease: open ? "out(3)" : "in(3)",
      onComplete: done,
    });
    return () => {
      anim.cancel();
    };
  }, [open, axis, offset, enter, exit]);

  return ref;
}
