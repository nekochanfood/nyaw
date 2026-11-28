import { useRouter, useRouterState } from "@tanstack/react-router";
import { animate, createTimeline } from "animejs";
import { type ReactNode, useLayoutEffect, useRef } from "react";

import { isLocale } from "@/i18n/config";
import { useLoadingDone } from "@/layout/Loading";
import { reducedMotion } from "@/motion/reducedMotion";
import { setPageShown, waitSettled } from "@/motion/stage";
import { scrollToTop } from "@/motion/useSmoothScroll";

/** ページのフェードアウトとフェードインの時間（ms） */
const FADE_MS = 400;

/** 言語を除いたパス。言語だけが違う移動は同じページとみなし、フェードしない */
const page = (pathname: string) => {
  const [, first, ...rest] = pathname.split("/");
  return `/${(isLocale(first) ? rest : [first, ...rest]).filter(Boolean).join("/")}`;
};

/**
 * 遷移の進み具合を示す線。本文のカードの上端に出す。`line` は線の場所で、出入りのときに不透明度だけを変える。
 * `bar` は線の幅いっぱいの棒で、左へずらしておき、右へ送って進み具合を見せる。始めは素早く 18% まで出し、
 * そのあとは読み込みが終わるまで 85% へゆっくり近づける。終わったら端まで送り、少し置いて消す
 */
function progress(line: HTMLElement | null, bar: HTMLElement | null) {
  if (!line || !bar || reducedMotion()) return { finish: () => {} };
  const timeline = createTimeline()
    .add(line, { opacity: [0, 1], duration: 150 }, 0)
    .add(bar, { x: ["-100%", "-82%"], duration: 200, ease: "out(2)" }, 0)
    .add(bar, { x: "-15%", duration: 8000, ease: "out(1)" });
  return {
    finish: () => {
      timeline.cancel();
      createTimeline()
        .add(line, { opacity: 1, duration: 100 }, 0)
        .add(bar, { x: "0%", duration: 180, ease: "out(2)" }, 0)
        .add(line, { opacity: 0, duration: 200 }, "+=50")
        .set(bar, { x: "-100%" });
    },
  };
}

/**
 * ページの出入りの動き。遷移の前に今のページをフェードアウトし、次のページの描画が落ち着いてからフェードインする。
 * フェードインを始めると同時に表示済み（setPageShown）にして、ページの中の動きを始めさせる。
 */
export function PageTransition({ children }: { children: ReactNode }) {
  const router = useRouter();
  const loaded = useLoadingDone();
  const key = useRouterState({ select: (s) => page((s.resolvedLocation ?? s.location).pathname) });
  const ref = useRef<HTMLDivElement>(null);
  const lineRef = useRef<HTMLDivElement>(null);
  const barRef = useRef<HTMLDivElement>(null);
  const finishProgress = useRef(() => {});
  const first = useRef(true);

  // 出る: ページを替える移動（リンク、戻る・進む）を、フェードアウトが終わるまで止めておく
  useLayoutEffect(
    () =>
      router.history.block({
        enableBeforeUnload: false,
        blockerFn: async ({ currentLocation, nextLocation }) => {
          if (page(nextLocation.pathname) === page(currentLocation.pathname)) return false;
          setPageShown(false);
          finishProgress.current = progress(lineRef.current, barRef.current).finish;
          const el = ref.current;
          if (el && !reducedMotion()) {
            // フェードアウトと同じ時間をかけて、ページの先頭へ滑らかに戻る。次のページは先頭から始まる
            scrollToTop(FADE_MS);
            await animate(el, { opacity: 0, y: 40, duration: FADE_MS, ease: "in(2)" });
          }
          return false;
        },
      }),
    [router],
  );

  // 入る: 読み込み画面が消えたとき（最初のページ）と、ページが替わったとき
  useLayoutEffect(() => {
    const el = ref.current;
    if (!loaded || !el) return;
    const isFirst = first.current;
    first.current = false;
    if (!isFirst) scrollToTop();
    if (reducedMotion()) {
      finishProgress.current();
      setPageShown(true);
      return;
    }
    el.style.opacity = "0";
    const abort = new AbortController();
    let anim: ReturnType<typeof animate> | undefined;
    void waitSettled(router, abort.signal).then(() => {
      finishProgress.current();
      anim = animate(el, { opacity: [0, 1], y: [40, 0], duration: FADE_MS, ease: "out(2)" });
      // 中の動きはフェードインと重ねて始める（終わるのを待つと、動き出しが遅く見える）
      setPageShown(true);
    });
    return () => {
      abort.abort();
      anim?.cancel();
    };
  }, [key, loaded, router]);

  return (
    <>
      {/*
        線の場所。左へずらした棒や先端の光は帯の内側の外へはみ出すが、線は帯の下の重なりにあるので、
        はみ出した部分は帯に覆われる（styles.css の --layer-progress）
      */}
      <div
        ref={lineRef}
        aria-hidden
        className="frame-x pointer-events-none fixed inset-x-0 top-(--frame-top) z-(--layer-progress) h-0.5 opacity-0"
      >
        <div
          ref={barRef}
          className="relative h-full w-full rounded-full bg-ctp-lavender text-ctp-lavender"
          style={{ transform: "translateX(-100%)" }}
        >
          {/* 先端の光。線の色のぼかした帯を、棒の右端に重ねる */}
          <span className="absolute top-1/2 -right-4 h-3 w-32 -translate-y-1/2 rounded-full bg-[linear-gradient(90deg,transparent_0%,currentColor_82%,transparent_100%)] opacity-80 blur-[4px]" />
        </div>
      </div>
      <div ref={ref} data-page className="flex grow flex-col">
        {children}
      </div>
    </>
  );
}
