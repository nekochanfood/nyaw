import { BellRinging } from "@phosphor-icons/react/dist/csr/BellRinging";
import { Compass } from "@phosphor-icons/react/dist/csr/Compass";
import { HandPalm } from "@phosphor-icons/react/dist/csr/HandPalm";
import { Keyboard } from "@phosphor-icons/react/dist/csr/Keyboard";
import { Monitor } from "@phosphor-icons/react/dist/csr/Monitor";
import { MusicNotes } from "@phosphor-icons/react/dist/csr/MusicNotes";
import { Palette } from "@phosphor-icons/react/dist/csr/Palette";
import { VirtualReality } from "@phosphor-icons/react/dist/csr/VirtualReality";
import { useRef } from "react";

import { useT } from "@/i18n/index";
import { useReveal } from "@/motion/useReveal";
import { containerClass, cx, textClass } from "@/ui/variants";

/** 機能と、そのアイコンの面の色（Catppuccin のアクセント） */
const FEATURES = [
  { key: "mirroring", icon: Monitor, tint: "bg-ctp-sky/15 text-ctp-sky" },
  { key: "notifications", icon: BellRinging, tint: "bg-ctp-peach/15 text-ctp-peach" },
  { key: "media", icon: MusicNotes, tint: "bg-ctp-pink/15 text-ctp-pink" },
  { key: "keyboard", icon: Keyboard, tint: "bg-ctp-green/15 text-ctp-green" },
  { key: "hands", icon: HandPalm, tint: "bg-ctp-yellow/15 text-ctp-yellow" },
  { key: "runtimes", icon: VirtualReality, tint: "bg-ctp-lavender/15 text-ctp-lavender" },
  { key: "appearance", icon: Palette, tint: "bg-ctp-mauve/15 text-ctp-mauve" },
  { key: "guides", icon: Compass, tint: "bg-ctp-teal/15 text-ctp-teal" },
] as const;

export function Features() {
  const t = useT("features");
  const ref = useRef<HTMLElement>(null);
  useReveal(ref);
  return (
    <section
      id="features"
      ref={ref}
      aria-labelledby="features-title"
      className={cx(containerClass, "py-24 sm:py-32")}
    >
      <div data-reveal-group>
        <h2 data-reveal id="features-title" className={textClass("section")}>
          {t("title")}
        </h2>
      </div>
      <ul data-reveal-group className="mt-12 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        {FEATURES.map(({ key, icon: Icon, tint }) => (
          <li key={key} data-reveal className="flex flex-col gap-4 rounded-xl bg-control/60 p-6">
            <span
              className={cx("inline-flex size-11 items-center justify-center rounded-lg", tint)}
            >
              <Icon size={24} weight="duotone" aria-hidden />
            </span>
            <h3 className={textClass("card")}>{t(`${key}_title`)}</h3>
            <p className={textClass("muted")}>{t(`${key}_body`)}</p>
          </li>
        ))}
      </ul>
    </section>
  );
}
