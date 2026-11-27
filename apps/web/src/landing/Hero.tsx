import { DownloadSimple } from "@phosphor-icons/react/dist/csr/DownloadSimple";
import { GithubLogo } from "@phosphor-icons/react/dist/csr/GithubLogo";
import { PawPrint } from "@phosphor-icons/react/dist/csr/PawPrint";
import { useRef } from "react";

import { Composite, type Layer } from "@/composite/Composite";
import { useLocale, useT } from "@/i18n/index";
import { REPO_URL } from "@/links";
import { useReveal } from "@/motion/useReveal";
import { ButtonAnchor, ButtonLink } from "@/ui/Button";
import { containerClass, cx, textClass } from "@/ui/variants";

/** 正面のミラーリングの画面、手前のダッシュボードと腕時計、奥に外したクイック設定 */
const LAYERS: Layer[] = [
  {
    role: "quick-settings",
    moment: "quick-settings",
    left: 58,
    top: 2,
    width: 40,
    z: -160,
    rotateY: -24,
  },
  { role: "mirroring", moment: "mirroring", left: 12, top: 4, width: 62, z: 0, rotateY: 6 },
  { role: "dashboard", moment: "mirroring", left: -6, top: 66, width: 92, z: 90, rotateX: 18 },
  { role: "wrist", moment: "mirroring", left: 68, top: 56, width: 22, z: 180, rotateY: -14 },
];

export function Hero() {
  const t = useT("hero");
  const locale = useLocale();
  const ref = useRef<HTMLElement>(null);
  useReveal(ref);
  return (
    <section ref={ref} className={cx(containerClass, "pt-12 pb-8 sm:pt-20 lg:pt-24")}>
      <div data-reveal-group className="flex max-w-3xl flex-col items-start gap-6">
        <p
          data-reveal
          className="flex items-center gap-2 rounded-full bg-ctp-lavender/15 px-3 py-1 text-sm font-medium"
        >
          <PawPrint aria-hidden size={16} weight="fill" className="text-ctp-lavender" />
          {t("eyebrow")}
        </p>
        <h1 data-reveal className={textClass("display")}>
          {t("title")}
        </h1>
        <p data-reveal className={cx(textClass("lead"), "max-w-xl")}>
          {t("body")}
        </p>
        <div data-reveal className="flex flex-wrap gap-3 pt-2">
          <ButtonLink to="/$locale/download" params={{ locale }} variant="solid" size="lg">
            <DownloadSimple size={20} weight="bold" />
            {t("download")}
          </ButtonLink>
          <ButtonAnchor href={REPO_URL} target="_blank" rel="noopener" variant="surface" size="lg">
            <GithubLogo size={20} />
            {t("github")}
          </ButtonAnchor>
        </div>
      </div>
      <div data-reveal-group>
        <div
          data-reveal
          className="mt-14 overflow-hidden rounded-2xl bg-linear-135 from-ctp-lavender/20 via-ctp-pink/15 to-ctp-peach/15 px-4 py-8 sm:px-10 sm:py-14"
        >
          <Composite
            layers={LAYERS}
            aspect={16 / 10}
            tilt={{ from: [6, -8], to: [12, 10] }}
            label={t("scene_label")}
            className="mx-auto max-w-4xl"
          />
        </div>
      </div>
    </section>
  );
}
