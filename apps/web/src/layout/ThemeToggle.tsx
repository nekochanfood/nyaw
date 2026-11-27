import { Toggle } from "@base-ui/react/toggle";
import { Moon } from "@phosphor-icons/react/dist/csr/Moon";
import { Sun } from "@phosphor-icons/react/dist/csr/Sun";
import { animate } from "animejs";
import { useLayoutEffect, useRef } from "react";

import { useT } from "@/i18n/index";
import { reducedMotion } from "@/motion/reducedMotion";
import { setTheme, useTheme } from "@/theme";
import { Tooltip } from "@/ui/Popup";
import { buttonClass } from "@/ui/variants";

export function ThemeToggle() {
  const t = useT("nav");
  const theme = useTheme();
  const iconRef = useRef<HTMLSpanElement>(null);
  const shown = useRef(theme);

  // アイコンが替わるたびに、回転しながら入る。最初の表示では回さない
  useLayoutEffect(() => {
    const was = shown.current;
    shown.current = theme;
    if (!was || !theme || was === theme || !iconRef.current || reducedMotion()) return;
    const anim = animate(iconRef.current, {
      rotate: [theme === "dark" ? -360 : -180, 0],
      duration: 900,
      ease: "out(4)",
    });
    return () => {
      anim.cancel();
    };
  }, [theme]);

  // サーバーではテーマが分からないので、同じ大きさの空きだけ置く
  if (!theme) return <span className="size-10" />;
  return (
    <Tooltip label={t("theme")}>
      <Toggle
        aria-label={t("theme")}
        pressed={theme === "dark"}
        onPressedChange={(dark) => setTheme(dark ? "dark" : "light")}
        className={buttonClass()}
      >
        <span ref={iconRef} className="inline-flex">
          {theme === "dark" ? <Moon size={20} weight="fill" /> : <Sun size={20} weight="fill" />}
        </span>
      </Toggle>
    </Tooltip>
  );
}
