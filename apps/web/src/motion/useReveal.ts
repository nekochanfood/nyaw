import { animate, onScroll, stagger } from "animejs";
import { type RefObject, useEffect } from "react";

import { reducedMotion } from "@/motion/reducedMotion";
import { usePageShown } from "@/motion/stage";

/** スクロールで現れる要素の動き。下から浮かび上がる。並んだ要素は `REVEAL_STAGGER`（ms）ずつずらす */
export const REVEAL = { opacity: [0, 1], y: [24, 0], duration: 700, ease: "out(3)" };
export const REVEAL_STAGGER = 80;

/**
 * `ref` の中の `[data-reveal]` を、画面の下から 85% の位置まで来たときに順に浮かび上がらせる（1 回だけ）。
 * ページのフェードインと同時に始める。それまでは styles.css が隠している（JavaScript が動き、
 * 動きを減らす設定でないときだけ）。
 */
export function useReveal(ref: RefObject<HTMLElement | null>) {
  const shown = usePageShown();
  useEffect(() => {
    const root = ref.current;
    if (!shown || !root || reducedMotion()) return;
    const anims = [...root.querySelectorAll<HTMLElement>("[data-reveal-group]")].map((group) =>
      animate(group.querySelectorAll("[data-reveal]"), {
        ...REVEAL,
        delay: stagger(REVEAL_STAGGER),
        autoplay: onScroll({ target: group, enter: "85% top", repeat: false }),
      }),
    );
    return () => {
      for (const anim of anims) anim.cancel();
    };
  }, [ref, shown]);
}
