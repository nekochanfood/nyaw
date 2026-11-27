import { useEffect } from "react";

import { reducedMotion } from "@/motion/reducedMotion";

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
      destroy = () => lenis.destroy();
    });
    return () => {
      destroyed = true;
      destroy();
    };
  }, []);
}
