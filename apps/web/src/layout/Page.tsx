import { animate, stagger } from "animejs";
import { type ReactNode, useLayoutEffect, useRef } from "react";

import { reducedMotion } from "@/motion/reducedMotion";
import { containerClass, cx } from "@/ui/variants";

/** 文字（書記素）ごとに分ける。分けた文字は React が描くので、文字の差し替えと競合しない */
const graphemes = (text: string) =>
  [...new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(text)].map(
    (s) => s.segment,
  );

/** 下位のページの上部。ページの名前を大きく置き、1 文字ずつ下から上がってくる */
export function PageTitle({ title }: { title: string }) {
  const ref = useRef<HTMLHeadingElement>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || reducedMotion()) return;
    const anim = animate(el.querySelectorAll("[data-char]"), {
      y: ["110%", "0%"],
      duration: 700,
      delay: stagger(30, { start: 150 }),
      ease: "out(4)",
    });
    return () => {
      anim.revert();
    };
  }, [title]);
  return (
    <div className={cx(containerClass, "flex h-56 items-end pb-8 md:h-96 md:pb-12")}>
      <h1
        ref={ref}
        aria-label={title}
        className="text-5xl font-bold tracking-tight sm:text-6xl md:text-8xl"
      >
        {graphemes(title).map((char, i) => (
          // 下に隠れた位置から上がるので、文字ごとに下を切り取る
          <span key={`${i}-${char}`} aria-hidden className="inline-block overflow-clip pb-[0.1em]">
            <span data-char className="inline-block whitespace-pre">
              {char}
            </span>
          </span>
        ))}
      </h1>
    </div>
  );
}

/** ページの名前の下に続く、本文の区画。地より一段沈んだ色にする */
export function PageSection({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div className="min-h-[720px] grow bg-background">
      <div className={cx(containerClass, "flex flex-col gap-12 pt-10 pb-24", className)}>
        {children}
      </div>
    </div>
  );
}
