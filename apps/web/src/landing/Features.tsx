import { BellRinging } from "@phosphor-icons/react/dist/csr/BellRinging";
import { Compass } from "@phosphor-icons/react/dist/csr/Compass";
import { HandPalm } from "@phosphor-icons/react/dist/csr/HandPalm";
import { Keyboard } from "@phosphor-icons/react/dist/csr/Keyboard";
import { Monitor } from "@phosphor-icons/react/dist/csr/Monitor";
import { MusicNotes } from "@phosphor-icons/react/dist/csr/MusicNotes";
import { Palette } from "@phosphor-icons/react/dist/csr/Palette";
import { VirtualReality } from "@phosphor-icons/react/dist/csr/VirtualReality";
import { animate } from "animejs";

import { useT } from "@/i18n/index";
import { reducedMotion } from "@/motion/reducedMotion";
import { cx, SURFACE, textClass } from "@/ui/variants";

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

/** カードをホバーしたときに、アイコンが小さく跳ねる */
function hop(card: HTMLElement) {
  const icon = card.querySelector("[data-icon]");
  if (!icon || reducedMotion()) return;
  animate(icon, {
    y: [0, -6, 0],
    rotate: [0, -8, 0],
    duration: 500,
    ease: "out(3)",
    composition: "replace",
  });
}

/** 機能の一覧。アイコンと短い説明のカードを並べる */
export function Features() {
  const t = useT("features");
  return (
    <ul data-reveal-group className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
      {FEATURES.map(({ key, icon: Icon, tint }) => (
        // oxlint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- 見た目だけの動き。操作はない
        <li
          key={key}
          data-reveal
          onMouseEnter={(e) => hop(e.currentTarget)}
          className={cx(SURFACE.card, "flex flex-col gap-4 p-6")}
        >
          <span
            data-icon
            className={cx("inline-flex size-12 items-center justify-center rounded-xl", tint)}
          >
            <Icon size={26} weight="duotone" aria-hidden />
          </span>
          <h2 className={textClass("card")}>{t(`${key}_title`)}</h2>
          <p className={textClass("muted")}>{t(`${key}_body`)}</p>
        </li>
      ))}
    </ul>
  );
}
