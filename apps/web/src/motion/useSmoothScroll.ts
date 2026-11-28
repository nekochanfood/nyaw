import { useEffect } from "react";

import type Lenis from "lenis";

import { reducedMotion } from "@/motion/reducedMotion";

let current: Lenis | null = null;

/**
 * ページの先頭へ移る。`duration`（ms）を渡すと、その時間をかけて滑らかに戻る（lenis が動いているときだけ）。
 * 渡さないときや、lenis が動いていないときはすぐに移る
 */
export function scrollToTop(duration?: number) {
  if (!current) return window.scrollTo(0, 0);
  if (duration) current.scrollTo(0, { duration: duration / 1000, force: true });
  else current.scrollTo(0, { immediate: true, force: true });
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
