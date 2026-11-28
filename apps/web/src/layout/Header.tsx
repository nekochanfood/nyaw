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
import { MENU_ITEM_SHIFT, MobileMenu } from "@/layout/MobileMenu";
import { useNavItems } from "@/layout/nav";
import { ThemeToggle } from "@/layout/ThemeToggle";

/** ロゴをホバーしたときに、首をかしげるように小さく揺らす */
function wiggle(el: Element | null) {
  if (!el || reducedMotion()) return;
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
      y: menuOpen ? -MENU_ITEM_SHIFT : 0,
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
    <header ref={ref} className="frame-x sticky top-0 z-(--layer-header) h-(--frame-top)">
      {/*
        上辺の帯（styles.css の --frame-top）に載せる。面は持たず、帯の色がそのまま見える。
        高さ 64px に 40px のボタンを置くので、上下の余白は 12px。左右も 12px にする。
        ロゴは、左端に来る幅（md 以上）では自分の左の余白で本文の端にそろえる
      */}
      <div className="flex h-full w-full items-center justify-between gap-2 px-3">
        <div className="flex items-center gap-2">
          {/* 狭い画面のメニューのボタン。ロゴの左隣に置く */}
          <div data-header-item className="md:hidden">
            <MobileMenu open={menuOpen} setOpen={setMenuOpen} />
          </div>
          <Link
            data-header-item
            data-menu-hide
            to="/$locale"
            params={{ locale }}
            aria-label={t("home")}
            // 左の余白は、ヘッダーの余白（12px）と足して本文の端（sm 以上で 32px）になる幅
            className="mr-2 flex h-10 items-center rounded-lg px-1 md:pl-5"
            onMouseEnter={(e) => wiggle(e.currentTarget.querySelector("[data-logo]"))}
          >
            {/* 揺らすのはロゴの絵だけ。余白込みの箱を回すと、回転の中心がずれる */}
            <span data-logo className="inline-flex">
              <Logo kind="mini" className="h-8 w-auto" />
            </span>
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
                variant="onFrame"
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
              <ButtonAnchor
                href={REPO_URL}
                target="_blank"
                rel="noopener"
                aria-label={t("github")}
                variant="onFrame"
              >
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
