import { ArrowRight } from "@phosphor-icons/react/dist/csr/ArrowRight";
import { PawPrint } from "@phosphor-icons/react/dist/csr/PawPrint";
import { useRef } from "react";

import { Composite, type Layer, type Tilt } from "@/composite/Composite";
import { useLocale, useT } from "@/i18n/index";
import { useReveal } from "@/motion/useReveal";
import { ButtonLink } from "@/ui/Button";
import { containerClass, cx, textClass } from "@/ui/variants";

const BAR = { role: "dashboard", moment: "mirroring" } as const;

/** 機能ごとの絵と、その背景の色（Catppuccin のアクセント） */
const SCENES: {
  key: "dashboard" | "mirroring" | "quick_settings" | "wrist";
  tint: string;
  paw: string;
  aspect: number;
  tilt: Tilt;
  layers: Layer[];
}[] = [
  {
    key: "dashboard",
    tint: "bg-ctp-lavender/15",
    paw: "text-ctp-lavender",
    aspect: 4 / 3,
    tilt: { from: [24, -14], to: [14, 8] },
    layers: [
      { ...BAR, left: -18, top: 52, width: 136, z: 40, rotateX: 10 },
      { role: "wrist", moment: "mirroring", left: 60, top: 16, width: 26, z: 120, rotateY: -12 },
    ],
  },
  {
    key: "mirroring",
    tint: "bg-ctp-sky/15",
    paw: "text-ctp-sky",
    aspect: 4 / 3,
    tilt: { from: [10, 16], to: [4, -6] },
    layers: [
      { role: "mirroring", moment: "mirroring", left: 9, top: 6, width: 82, z: 0 },
      { ...BAR, left: -2, top: 72, width: 104, z: 80, rotateX: 16 },
    ],
  },
  {
    key: "quick_settings",
    tint: "bg-ctp-peach/15",
    paw: "text-ctp-peach",
    aspect: 4 / 3,
    tilt: { from: [10, -16], to: [4, 6] },
    layers: [
      { role: "quick-settings", moment: "quick-settings", left: 9, top: 6, width: 82, z: 0 },
      { ...BAR, left: -2, top: 72, width: 104, z: 80, rotateX: 16 },
    ],
  },
  {
    key: "wrist",
    tint: "bg-ctp-pink/15",
    paw: "text-ctp-pink",
    aspect: 4 / 3,
    tilt: { from: [14, 18], to: [6, -10] },
    layers: [{ role: "wrist", moment: "mirroring", left: 22, top: 8, width: 56, z: 60 }],
  },
];

/** 主な機能を、パネルを立体的に重ねた絵と一緒に 1 つずつ紹介する */
export function Showcase() {
  const t = useT("tour");
  const features = useT("features");
  const locale = useLocale();
  const ref = useRef<HTMLElement>(null);
  useReveal(ref);
  return (
    <section ref={ref} aria-labelledby="showcase-title" className={cx(containerClass, "py-16")}>
      <div data-reveal-group>
        <h2 data-reveal id="showcase-title" className={textClass("section")}>
          {t("title")}
        </h2>
      </div>
      <div className="mt-12 flex flex-col gap-6">
        {SCENES.map((scene, i) => (
          <article
            key={scene.key}
            data-reveal-group
            className="grid items-center gap-8 rounded-2xl bg-control/50 p-4 sm:p-8 lg:grid-cols-2 lg:gap-12"
          >
            <div className={cx("flex flex-col gap-4 p-2 sm:p-0", i % 2 === 1 && "lg:order-2")}>
              <PawPrint
                data-reveal
                aria-hidden
                size={28}
                weight="fill"
                className={cx(scene.paw, "-rotate-12")}
              />
              <h3 data-reveal className="text-2xl font-bold sm:text-3xl">
                {t(`${scene.key}_title`)}
              </h3>
              <p data-reveal className={cx(textClass("lead"), "max-w-md")}>
                {t(`${scene.key}_body`)}
              </p>
            </div>
            {/* 外側のカードと角を同心にする（半径 = 外側の半径 - 余白） */}
            <div
              data-reveal
              className={cx(
                "overflow-hidden rounded-[calc(var(--radius-2xl)-1rem)] p-4 sm:rounded-[calc(var(--radius-2xl)-2rem)] sm:p-8",
                scene.tint,
              )}
            >
              <Composite
                layers={scene.layers}
                aspect={scene.aspect}
                tilt={scene.tilt}
                label={t(`${scene.key}_title`)}
              />
            </div>
          </article>
        ))}
      </div>
      <div data-reveal-group className="mt-10 flex justify-center">
        <ButtonLink
          data-reveal
          to="/$locale/features"
          params={{ locale }}
          variant="surface"
          size="lg"
        >
          {features("more")}
          <ArrowRight size={20} weight="bold" />
        </ButtonLink>
      </div>
    </section>
  );
}
