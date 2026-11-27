import { useEffect } from "react";

import type Lenis from "lenis";

import { reducedMotion } from "@/motion/reducedMotion";

let current: Lenis | null = null;

/** ページの先頭へすぐに移る。lenis が動いていれば lenis に、なければブラウザに頼む */
export function scrollToTop() {
  if (current) current.scrollTo(0, { immediate: true, force: true });
  else window.scrollTo(0, 0);
}

/** ページのスクロールを lenis で滑らかにする。視差効果を減らす設定のときは使わない */
export function useSmoothScroll() {
  useEffect(() => {
    if (reducedMotion()) return;
    let destroyed = false;
    let destroy = () => {};
    void import("lenis").then(({ default: Lenis }) => {
      if (destroyed) return;
      // ページ内リンクも滑らかに移動する。飛び先はヘッダー（h-16）と帯の分だけ下げる
      const lenis = new Lenis({ autoRaf: true, anchors: { offset: -80 } });
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
