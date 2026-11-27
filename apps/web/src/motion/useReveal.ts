import { animate, onScroll, stagger } from "animejs";
import { type RefObject, useEffect } from "react";

import { reducedMotion } from "@/motion/reducedMotion";

/**
 * `ref` の中の `[data-reveal]` を、画面の下から 85% の位置まで来たときに順に浮かび上がらせる（1 回だけ）。
 * JavaScript が動かない環境でも読めるよう、隠すのはこの処理が動いたときだけにする。
 */
export function useReveal(ref: RefObject<HTMLElement | null>) {
  useEffect(() => {
    const root = ref.current;
    if (!root || reducedMotion()) return;
    const anims = [...root.querySelectorAll<HTMLElement>("[data-reveal-group]")].map((group) =>
      animate(group.querySelectorAll("[data-reveal]"), {
        opacity: [0, 1],
        y: [24, 0],
        duration: 700,
        delay: stagger(80),
        ease: "out(3)",
        autoplay: onScroll({ target: group, enter: "85% top", repeat: false }),
      }),
    );
    return () => {
      for (const anim of anims) anim.revert();
    };
  }, [ref]);
}
