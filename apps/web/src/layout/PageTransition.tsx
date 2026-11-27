import { useRouter, useRouterState } from "@tanstack/react-router";
import { animate, createTimeline } from "animejs";
import { type ReactNode, useLayoutEffect, useRef } from "react";

import { isLocale } from "@/i18n/config";
import { useLoadingDone } from "@/layout/Loading";
import { reducedMotion } from "@/motion/reducedMotion";
import { setPageShown, waitSettled } from "@/motion/stage";
import { scrollToTop } from "@/motion/useSmoothScroll";

/** 言語を除いたパス。言語だけが違う移動は同じページとみなし、フェードしない */
const page = (pathname: string) => {
  const [, first, ...rest] = pathname.split("/");
  return `/${(isLocale(first) ? rest : [first, ...rest]).filter(Boolean).join("/")}`;
};

/** 遷移の進み具合を示す線。画面の上端の帯の内側に出す */
function progress(bar: HTMLElement | null) {
  if (!bar || reducedMotion()) return { finish: () => {} };
  const timeline = createTimeline()
    .add(bar, { opacity: [0, 1], duration: 150 }, 0)
    .add(bar, { scaleX: [0, 0.2], duration: 200, ease: "out(2)" }, 0)
    .add(bar, { scaleX: 0.85, duration: 6000, ease: "out(1)" });
  return {
    finish: () => {
      timeline.cancel();
      createTimeline()
        .add(bar, { scaleX: 1, duration: 180, ease: "out(2)" })
        .add(bar, { opacity: 0, duration: 200 }, "+=50")
        .set(bar, { scaleX: 0 });
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
          finishProgress.current = progress(barRef.current).finish;
          const el = ref.current;
          if (el && !reducedMotion()) {
            await animate(el, { opacity: 0, y: 40, duration: 400, ease: "in(2)" });
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
      anim = animate(el, { opacity: [0, 1], y: [40, 0], duration: 400, ease: "out(2)" });
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
      <div
        ref={barRef}
        aria-hidden
        className="pointer-events-none fixed inset-x-(--frame-gutter) top-(--frame-gutter) z-50 h-0.5 origin-left rounded-full bg-ctp-lavender opacity-0"
        style={{ transform: "scaleX(0)" }}
      />
      <div ref={ref} data-page className="flex grow flex-col">
        {children}
      </div>
    </>
  );
}
