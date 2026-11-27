import { GithubLogo } from "@phosphor-icons/react/dist/csr/GithubLogo";
import { Link } from "@tanstack/react-router";
import { animate, stagger } from "animejs";
import { useLayoutEffect, useRef, useState } from "react";

import { useLocale, useT } from "@/i18n/index";
import { REPO_URL } from "@/links";
import { reducedMotion } from "@/motion/reducedMotion";
import { ButtonAnchor, ButtonLink } from "@/ui/Button";
import { Tooltip } from "@/ui/Popup";
import { LanguageMenu } from "@/layout/LanguageMenu";
import { Logo } from "@/layout/Logo";
import { useLoadingDone } from "@/layout/Loading";
import { MobileMenu } from "@/layout/MobileMenu";
import { ThemeToggle } from "@/layout/ThemeToggle";

/** ヘッダーとモバイルのメニューに並べるページ */
export function useNavItems() {
  const t = useT("nav");
  const locale = useLocale();
  return [
    { label: t("features"), to: "/$locale/features", hash: undefined, params: { locale } },
    { label: t("download"), to: "/$locale/download", hash: undefined, params: { locale } },
  ] as const;
}

/** ロゴをホバーしたときに、首をかしげるように小さく揺らす */
function wiggle(el: HTMLElement) {
  if (reducedMotion()) return;
  animate(el, {
    rotate: [0, -10, 8, -4, 0],
    duration: 700,
    ease: "out(2)",
    composition: "replace",
  });
}

export function Header() {
  const t = useT("nav");
  const locale = useLocale();
  const items = useNavItems();
  const ref = useRef<HTMLElement>(null);
  const loaded = useLoadingDone();
  const [menuOpen, setMenuOpen] = useState(false);
  const menuWasOpen = useRef(false);

  // 狭い画面のメニューを開いている間は、ロゴと右のボタンを上へ引っ込める。閉じたら戻す
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || menuWasOpen.current === menuOpen) return;
    menuWasOpen.current = menuOpen;
    const targets = [...el.querySelectorAll<HTMLElement>("[data-menu-hide]")];
    // 引っ込めている間は、フォーカスも当たらないようにする
    for (const target of targets) target.inert = menuOpen;
    if (reducedMotion()) {
      for (const target of targets) target.style.visibility = menuOpen ? "hidden" : "";
      return;
    }
    const anim = animate(targets, {
      opacity: menuOpen ? 0 : 1,
      y: menuOpen ? -24 : 0,
      duration: menuOpen ? 250 : 500,
      delay: stagger(40),
      ease: menuOpen ? "in(2)" : "out(3)",
    });
    return () => {
      anim.cancel();
    };
  }, [menuOpen]);

  // 読み込み画面が消えたら、ページのフェードイン（PageTransition）に少し遅れて、項目が上から順に降りてくる
  useLayoutEffect(() => {
    const el = ref.current;
    if (!loaded || !el || reducedMotion()) return;
    // 登場が終わったら、項目を隠す CSS（styles.css の .motion）を外す。あとで作り直された項目も見えるようにする
    const ready = () => (document.documentElement.dataset.headerReady = "");
    const anim = animate(el.querySelectorAll("[data-header-item]"), {
      opacity: [0, 1],
      y: [-32, 0],
      duration: 800,
      delay: stagger(50, { start: 300 }),
      ease: "out(3)",
      onComplete: ready,
    });
    return () => {
      anim.cancel();
      ready();
    };
  }, [loaded]);

  return (
    <header ref={ref} className="header-band sticky top-(--frame-gutter) z-30 h-16">
      {/*
        スマートフォンの幅では、高さ 64px に 40px のボタンを置いた上下の余白（12px）と左右の余白をそろえる。
        それより広い幅では、本文と同じ余白（containerClass）にして、項目の端を本文の端にそろえる。
        左端の項目は、ボタンの中の余白の分だけ外へ出し、アイコンやロゴの見た目の端を本文の端に合わせる
      */}
      <div className="flex h-full w-full items-center justify-between gap-2 px-3 sm:px-8">
        <div className="flex items-center gap-2">
          {/* 狭い画面のメニューのボタン。ロゴの左隣に置く */}
          <div data-header-item className="sm:-ml-2.5 md:hidden">
            <MobileMenu open={menuOpen} setOpen={setMenuOpen} />
          </div>
          <Link
            data-header-item
            data-menu-hide
            to="/$locale"
            params={{ locale }}
            aria-label={t("home")}
            className="mr-2 rounded-lg p-1 md:-ml-1"
            onMouseEnter={(e) => wiggle(e.currentTarget)}
          >
            <Logo kind="mini" className="h-8 w-auto" />
          </Link>
          <nav className="hidden items-center gap-1 md:flex">
            {items.map((item) => (
              // key は言語で変わらないリンク先にする（文言にすると、言語を替えたときに作り直される）
              <ButtonLink
                key={item.to}
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
        <div data-menu-hide className="flex items-center gap-1">
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
        </div>
      </div>
    </header>
  );
}
