import { animate, onScroll } from "animejs";
import { type CSSProperties, useEffect, useRef } from "react";

import { useLocale } from "@/i18n/index";
import { reducedMotion } from "@/motion/reducedMotion";
import { cx } from "@/ui/variants";

/** アプリのキャプチャ（apps/nyaw の `site` のテスト）が書き出すパネルと、その場面 */
type Panel =
  | { role: "dashboard" | "mirroring" | "wrist"; moment: "mirroring" }
  | { role: "quick-settings"; moment: "quick-settings" };

/** 舞台に置くパネル。位置と幅は舞台の幅に対する %、奥行きは px、傾きは度 */
export type Layer = Panel & {
  left: number;
  top: number;
  width: number;
  z?: number;
  rotateX?: number;
  rotateY?: number;
};

export type Tilt = { from: [x: number, y: number]; to: [x: number, y: number] };

const src = (theme: "light" | "dark", locale: string, { moment, role }: Panel) =>
  `/scene/${theme}-${locale}/${moment}/${role}.png`;

type Props = {
  layers: Layer[];
  /** 舞台の縦横比（幅 / 高さ） */
  aspect: number;
  /** スクロールに合わせた舞台全体の傾き。画面の下端に入ってから上端を抜けるまでに from から to へ変わる */
  tilt: Tilt;
  label: string;
  className?: string;
};

/**
 * パネルのキャプチャを、奥行きと傾きを付けて重ねた絵。テーマの両方の絵を置き、表示を CSS で切り替える
 * （サーバーではテーマが分からない）。隠れている側は loading="lazy" なので読み込まれない。
 */
export function Composite({ layers, aspect, tilt, label, className }: Props) {
  const locale = useLocale();
  const rootRef = useRef<HTMLDivElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const middle = (i: 0 | 1) => (tilt.from[i] + tilt.to[i]) / 2;

  useEffect(() => {
    const [root, stage] = [rootRef.current, stageRef.current];
    if (!root || !stage || reducedMotion()) return;
    const anim = animate(stage, {
      rotateX: [tilt.from[0], tilt.to[0]],
      rotateY: [tilt.from[1], tilt.to[1]],
      ease: "linear",
      autoplay: onScroll({ target: root, enter: "bottom top", leave: "top bottom", sync: 0.25 }),
    });
    return () => {
      anim.revert();
    };
  }, [tilt]);

  return (
    <div
      ref={rootRef}
      // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- 重ねた何枚もの絵を、1 枚の画像として読み上げさせる
      role="img"
      aria-label={label}
      className={cx("relative [perspective:1600px]", className)}
      style={{ aspectRatio: aspect }}
    >
      <div
        ref={stageRef}
        className="absolute inset-0 transform-3d"
        style={{ transform: `rotateX(${middle(0)}deg) rotateY(${middle(1)}deg)` }}
      >
        {layers.map((layer) => {
          const style: CSSProperties = {
            left: `${layer.left}%`,
            top: `${layer.top}%`,
            width: `${layer.width}%`,
            transform: `translateZ(${layer.z ?? 0}px) rotateX(${layer.rotateX ?? 0}deg) rotateY(${layer.rotateY ?? 0}deg)`,
          };
          return (["light", "dark"] as const).map((theme) => (
            <img
              key={`${layer.role}-${theme}`}
              src={src(theme, locale, layer)}
              alt=""
              loading="lazy"
              decoding="async"
              draggable={false}
              className={cx(
                "absolute h-auto select-none",
                theme === "light" ? "dark:hidden" : "hidden dark:block",
              )}
              style={style}
            />
          ));
        })}
      </div>
    </div>
  );
}
