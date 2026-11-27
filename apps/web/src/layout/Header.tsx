import { GithubLogo } from "@phosphor-icons/react/dist/csr/GithubLogo";
import { Link } from "@tanstack/react-router";
import { createTimeline, stagger } from "animejs";
import { useLayoutEffect, useRef } from "react";

import { useLocale, useT } from "@/i18n/index";
import { REPO_URL } from "@/links";
import { reducedMotion } from "@/motion/reducedMotion";
import { ButtonAnchor, ButtonLink } from "@/ui/Button";
import { Tooltip } from "@/ui/Popup";
import { containerClass, cx } from "@/ui/variants";
import { LanguageMenu } from "@/layout/LanguageMenu";
import { Logo } from "@/layout/Logo";
import { MobileMenu } from "@/layout/MobileMenu";
import { ThemeToggle } from "@/layout/ThemeToggle";

/** ヘッダーとモバイルのメニューに並べるページ */
export function useNavItems() {
  const t = useT("nav");
  const locale = useLocale();
  return [
    { label: t("features"), to: "/$locale", hash: "features", params: { locale } },
    { label: t("download"), to: "/$locale/download", hash: undefined, params: { locale } },
  ] as const;
}

export function Header() {
  const t = useT("nav");
  const locale = useLocale();
  const items = useNavItems();
  const ref = useRef<HTMLElement>(null);

  // 開いたときに本文がフェードしながら上がり、続いてヘッダーの項目が上から順に降りてくる
  useLayoutEffect(() => {
    const el = ref.current;
    const main = document.getElementById("main");
    if (!el || !main || reducedMotion()) return;
    const timeline = createTimeline()
      .add(main, { opacity: [0, 1], y: [40, 0], duration: 500, ease: "out(2)" })
      .add(
        el.querySelectorAll("[data-header-item]"),
        { opacity: [0, 1], y: [-32, 0], duration: 800, delay: stagger(50), ease: "out(3)" },
        "-=200",
      );
    return () => {
      timeline.revert();
    };
  }, []);

  return (
    <header
      ref={ref}
      className="sticky top-(--frame-gutter) z-30 h-16 before:pointer-events-none before:absolute before:inset-x-0 before:top-0 before:-bottom-full before:-z-1 before:bg-card/80 before:mask-b-from-40% before:backdrop-blur-lg"
    >
      <div className={cx(containerClass, "flex h-full items-center justify-between gap-2")}>
        <div className="flex items-center gap-2">
          <Link
            data-header-item
            to="/$locale"
            params={{ locale }}
            aria-label={t("home")}
            className="mr-2 rounded-lg p-1"
          >
            <Logo className="h-7 w-auto" />
          </Link>
          <nav className="hidden items-center gap-1 md:flex">
            {items.map((item) => (
              <ButtonLink
                key={item.label}
                data-header-item
                to={item.to}
                params={item.params}
                hash={item.hash}
                size="md"
                className="font-normal"
              >
                {item.label}
              </ButtonLink>
            ))}
          </nav>
        </div>
        <div className="flex items-center gap-1">
          <div data-header-item className="hidden sm:block">
            <Tooltip label={t("github")}>
              <ButtonAnchor href={REPO_URL} target="_blank" rel="noopener" aria-label={t("github")}>
                <GithubLogo size={20} />
              </ButtonAnchor>
            </Tooltip>
          </div>
          <div data-header-item>
            <LanguageMenu />
          </div>
          <div data-header-item>
            <ThemeToggle />
          </div>
          <div data-header-item className="md:hidden">
            <MobileMenu />
          </div>
        </div>
      </div>
    </header>
  );
}
