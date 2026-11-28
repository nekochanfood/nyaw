import type { AnyRouter } from "@tanstack/react-router";

/*
  ページの出入りの順番を支える部品。PageTransition は、読み込み画面 → ページのフェードイン、の順に進め、遷移のときは
  フェードアウト → 次のページの描画が落ち着くのを待つ（waitSettled）→ フェードイン、と進める。
*/

/** これを超えたら、落ち着いていなくてもフェードインする（読み込みが遅いときに空白のまま待たせない） */
const MAX_WAIT_MS = 2000;
/** 平常の間隔のフレームがこれだけ続いたら、落ち着いたとみなす */
const QUIET_FRAMES = 3;

/**
 * 次のページの描画が落ち着くまで待つ。ルーターの読み込みが終わり、フレームの間隔が数フレーム続けて
 * 平常に戻ったら解決する。描画し直しや遅延読み込みがフェードインに重なると、フレームが落ちる。
 */
export function waitSettled(router: AnyRouter, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    const start = performance.now();
    let last = -1;
    let min = Infinity;
    let quiet = 0;
    const tick = (now: number) => {
      if (signal.aborted) return;
      // 最初の 1 回は呼び出しからの時間なので、間隔には数えない
      const dt = last < 0 ? Infinity : now - last;
      last = now;
      min = Math.min(min, dt);
      const busy = router.state.status === "pending";
      // 平常の間隔は見えた中で最短のもの。120Hz で揺れても待ち続けないよう 20ms までは許す
      quiet = !busy && dt < Math.max(min * 1.5, 20) ? quiet + 1 : 0;
      if (quiet >= QUIET_FRAMES || now - start > MAX_WAIT_MS) resolve();
      else requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
}

/**
 * 描画の前に `<html class="motion">` を付ける（`<head>` に埋め込む）。動きを減らす設定でなく、JavaScript が動く
 * ときだけ付き、動く要素は styles.css が最初から隠す。動きが始まる前に一瞬見えないようにするため。
 */
export const motionScript = `try{if(!matchMedia("(prefers-reduced-motion: reduce)").matches)document.documentElement.classList.add("motion")}catch(e){}`;
