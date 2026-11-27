import { onScroll } from "animejs";
import { useEffect, useRef } from "react";

import { useLocale, useT } from "@/i18n/index";
import { useReveal } from "@/motion/useReveal";
import { Scene } from "@/scene/Scene";
import { STEPS } from "@/scene/path";
import { useTheme } from "@/theme";
import { containerClass, cx, textClass } from "@/ui/variants";

/** 段ごとの印の色（Catppuccin のアクセント） */
const DOT = {
  dashboard: "bg-ctp-lavender",
  mirroring: "bg-ctp-sky",
  quick_settings: "bg-ctp-peach",
  wrist: "bg-ctp-pink",
} as const;

/**
 * パネルを並べた空間を画面に留めたまま、スクロールに合わせてカメラを各パネルへ寄せる。
 * 説明の文は横（狭い画面では下）を流れる。
 */
export function Tour() {
  const t = useT("tour");
  const hero = useT("hero");
  const locale = useLocale();
  const theme = useTheme();
  const ref = useRef<HTMLElement>(null);
  const progress = useRef(0);
  useReveal(ref);

  useEffect(() => {
    const section = ref.current;
    if (!section) return;
    // 区画の上端が画面の上端に来てから、下端が画面の下端に来るまでを 0〜1 とする
    const observer = onScroll({
      target: section,
      enter: "top top",
      leave: "bottom bottom",
      onUpdate: (self) => {
        progress.current = self.progress;
      },
    });
    return () => {
      observer.revert();
    };
  }, []);

  return (
    <section ref={ref} aria-labelledby="tour-title" className="relative">
      <div
        className={cx(containerClass, "grid gap-x-12 lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)]")}
      >
        <div className="sticky top-[calc(var(--frame-gutter)+4rem)] z-10 -mx-5 h-[48svh] bg-card sm:-mx-8 lg:order-2 lg:mx-0 lg:h-[calc(100svh-2*var(--frame-gutter)-5rem)]">
          <div className="size-full overflow-hidden lg:rounded-xl lg:bg-groove/40">
            {theme && (
              <Scene
                theme={theme}
                locale={locale}
                progress={progress}
                label={hero("scene_label")}
              />
            )}
          </div>
        </div>
        <div className="lg:order-1">
          <h2 id="tour-title" className={cx(textClass("section"), "pt-10 lg:pt-[20svh]")}>
            {t("title")}
          </h2>
          <ol>
            {STEPS.map((step) => (
              <li
                key={step}
                data-reveal-group
                className="flex min-h-[60svh] flex-col justify-center gap-3 py-12 lg:min-h-[85svh]"
              >
                <h3 data-reveal className="flex items-center gap-3 text-2xl font-bold">
                  <span aria-hidden className={cx("size-2.5 rounded-full", DOT[step])} />
                  {t(`${step}_title`)}
                </h3>
                <p data-reveal className={cx(textClass("lead"), "max-w-md")}>
                  {t(`${step}_body`)}
                </p>
              </li>
            ))}
          </ol>
        </div>
      </div>
    </section>
  );
}
