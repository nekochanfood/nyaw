import { useEffect } from "react";

import type Lenis from "lenis";

import { reducedMotion } from "@/motion/reducedMotion";

let current: Lenis | null = null;

/**
 * 時間を指定して戻るときの曲線。ゆっくり動き出し、ゆっくり止まる（3 次の ease-in-out）。lenis の既定の曲線
 * （expo の ease-out）は最初の数フレームで大半を進むので、短い距離では一瞬で飛んだように見える
 */
const easeInOutCubic = (t: number) => (t < 0.5 ? 4 * t ** 3 : 1 - (-2 * t + 2) ** 3 / 2);

/**
 * ページの先頭へ移る。`duration`（ms）を渡すと、その時間をかけて滑らかに戻る（lenis が動いているときだけ）。
 * 渡さないときや、lenis が動いていないときはすぐに移る
 */
export function scrollToTop(duration?: number) {
  if (!current) return window.scrollTo(0, 0);
  if (duration) {
    current.scrollTo(0, { duration: duration / 1000, easing: easeInOutCubic, force: true });
  } else {
    current.scrollTo(0, { immediate: true, force: true });
  }
}

/** ページのスクロールを lenis で滑らかにする。視差効果を減らす設定のときは使わない */
export function useSmoothScroll() {
  useEffect(() => {
    if (reducedMotion()) return;
    let destroyed = false;
    let destroy = () => {};
    void import("lenis").then(({ default: Lenis }) => {
      if (destroyed) return;
      // ページ内リンクも滑らかに移動する。飛び先は、ブラウザと同じく scroll-padding-top（styles.css）だけ下げる
      const padding = parseFloat(getComputedStyle(document.documentElement).scrollPaddingTop) || 0;
      const lenis = new Lenis({ autoRaf: true, anchors: { offset: -padding } });
      current = lenis;
      destroy = () => {
        lenis.destroy();
        current = null;
      };
    });
    return () => {
      destroyed = true;
      destroy();
    };
  }, []);
}
