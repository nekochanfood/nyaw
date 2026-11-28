import { ArrowSquareOut } from "@phosphor-icons/react/dist/csr/ArrowSquareOut";
import { GithubLogo } from "@phosphor-icons/react/dist/csr/GithubLogo";
import { Link } from "@tanstack/react-router";
import { createTimeline, onScroll, stagger, type Timeline } from "animejs";
import { type ReactNode, useEffect, useRef } from "react";

import { useLocale, useT } from "@/i18n/index";
import { ISSUES_URL, RELEASES_URL, REPO_URL } from "@/links";
import { reducedMotion } from "@/motion/reducedMotion";
import { usePageShown } from "@/motion/stage";
import { REVEAL, REVEAL_STAGGER } from "@/motion/useReveal";
import { ButtonAnchor } from "@/ui/Button";
import { buttonClass, containerClass, cx, textClass } from "@/ui/variants";
import { Logo } from "@/layout/Logo";

const linkClass = buttonClass({
  variant: "text",
  size: "none",
  className: "self-start py-1 text-sm",
});

function Column({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div data-reveal data-footer-side="left" className="flex min-w-36 flex-col gap-1.5">
      <h2 className="py-1 text-sm font-bold">{title}</h2>
      {children}
    </div>
  );
}

function External({ href, children }: { href: string; children: ReactNode }) {
  return (
    <a href={href} target="_blank" rel="noopener" className={linkClass}>
      {children}
      <ArrowSquareOut aria-hidden className="ml-1 inline align-[-0.125em]" />
    </a>
  );
}

/**
 * 帯と同じ色の面に置くフッター。本文のカードの下に続く。余白は上下左右とも本文の左右の余白（--content-pad-x）。
 * md 以上では、フッターの左右の端を上の本文のカードの端にそろえる（frame-x）。md 未満では画面の幅いっぱいに置く。
 * フッターは帯と同じ色なので、左右の帯の分も余白に見え、左右だけ広く見えるため
 */
export function Footer() {
  const t = useT("footer");
  const nav = useT("nav");
  const locale = useLocale();
  const ref = useRef<HTMLElement>(null);
  const shown = usePageShown();
  const timeline = useRef<Timeline | null>(null);

  /*
    画面に入ったら、本文のスクロールで現れる要素（useReveal）と同じ動きで浮かび上がる。左側（リンク、ライセンス）と
    右側（ロゴ、GitHub のボタン）は同時に動き出し、左側は上から、右側は下から順に現れる。フッターはページを移っても
    残るので、最初のページの表示のあとに 1 回だけ作る
  */
  useEffect(() => {
    const root = ref.current;
    if (!shown || timeline.current || !root || reducedMotion()) return;
    const side = (name: "left" | "right") => root.querySelectorAll(`[data-footer-side="${name}"]`);
    timeline.current = createTimeline({
      autoplay: onScroll({ target: root, enter: "85% top", repeat: false }),
    })
      .add(side("left"), { ...REVEAL, delay: stagger(REVEAL_STAGGER) }, 0)
      .add(side("right"), { ...REVEAL, delay: stagger(REVEAL_STAGGER, { from: "last" }) }, 0);
  }, [shown]);
  useEffect(() => () => void timeline.current?.cancel(), []);

  return (
    <footer ref={ref} className="w-full py-(--content-pad-x) md:frame-x">
      <div className={cx(containerClass, "flex flex-col gap-12")}>
        <div className="flex flex-col-reverse gap-10 sm:flex-row sm:justify-between">
          <div className="flex flex-wrap gap-x-16 gap-y-8">
            <Column title={t("title_product")}>
              <Link to="/$locale/features" params={{ locale }} className={linkClass}>
                {nav("features")}
              </Link>
              <Link to="/$locale/download" params={{ locale }} className={linkClass}>
                {nav("download")}
              </Link>
            </Column>
            <Column title={t("title_resources")}>
              <External href={REPO_URL}>{t("source")}</External>
              <External href={RELEASES_URL}>{t("releases")}</External>
              <External href={ISSUES_URL}>{t("issues")}</External>
            </Column>
          </div>
          <div data-reveal data-footer-side="right" className="self-start">
            <Logo className="h-20 w-auto sm:h-28" />
          </div>
        </div>
        <div className="flex items-end justify-between gap-6">
          <div className="flex flex-col gap-1">
            <p data-reveal data-footer-side="left" className={textClass("muted")}>
              {t("licence")}
            </p>
            <p data-reveal data-footer-side="left" className={textClass("muted")}>
              {t("copyright")}
            </p>
          </div>
          <ButtonAnchor
            href={REPO_URL}
            target="_blank"
            rel="noopener"
            aria-label={nav("github")}
            variant="onFrame"
            data-reveal
            data-footer-side="right"
          >
            <GithubLogo size={20} />
          </ButtonAnchor>
        </div>
      </div>
    </footer>
  );
}
