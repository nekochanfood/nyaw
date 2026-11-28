import { ArrowSquareOut } from "@phosphor-icons/react/dist/csr/ArrowSquareOut";
import { GithubLogo } from "@phosphor-icons/react/dist/csr/GithubLogo";
import { Link } from "@tanstack/react-router";
import { createTimeline } from "animejs";
import type { ReactNode } from "react";

import { useLocale, useT } from "@/i18n/index";
import { ISSUES_URL, RELEASES_URL, REPO_URL } from "@/links";
import { reducedMotion } from "@/motion/reducedMotion";
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
    <div className="flex flex-col gap-1.5">
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

/** しゃがんでから跳び上がって 1 回転し、着地でぷにっと弾む。動いている間の押下は無視する */
function spin(button: HTMLElement) {
  const body = button.firstElementChild as HTMLElement;
  if (reducedMotion() || "spinning" in button.dataset) return;
  button.dataset.spinning = "";
  createTimeline({ onComplete: () => delete button.dataset.spinning })
    .add(body, { scaleX: 1.15, scaleY: 0.82, duration: 120, ease: "out(2)" })
    .add(body, { y: -24, scaleX: 0.92, scaleY: 1.08, duration: 280, ease: "out(2)" })
    .add(body, { y: 0, scaleX: 1, scaleY: 1, duration: 280, ease: "in(2)" })
    .add(body, { scaleX: 1.18, scaleY: 0.82, duration: 90, ease: "out(2)" })
    .add(body, { scaleX: 1, scaleY: 1, duration: 600, ease: "outElastic(1, 0.4)" })
    // 回転は跳んでいる間に済ませる。伸び縮みは足元、回転は中心を軸にするので、要素を分ける
    .add(
      body.firstElementChild as HTMLElement,
      { rotate: [0, 360], duration: 560, ease: "inOut(2)" },
      120,
    );
}

/** 押すと回るミニロゴ */
function SpinningLogo() {
  return (
    <button
      type="button"
      aria-label="nyaw"
      // 押した反応は回転で見せるので、波紋（ui/ripple.ts）は出さない
      data-no-ripple
      className="block cursor-pointer rounded-lg"
      onClick={(e) => spin(e.currentTarget)}
    >
      <span className="block origin-bottom">
        <span className="flex">
          <Logo kind="mini" className="h-auto w-12" />
        </span>
      </span>
    </button>
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
  return (
    <footer className="w-full py-(--content-pad-x) md:frame-x">
      <div className={cx(containerClass, "flex flex-col gap-12")}>
        <div className="flex justify-between gap-10">
          <div className="flex flex-wrap gap-x-12 gap-y-8">
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
          <div className="max-sm:hidden">
            <SpinningLogo />
          </div>
        </div>
        <div className="flex items-end justify-between gap-6">
          <div className="flex flex-col gap-1">
            <div className="mb-3 sm:hidden">
              <SpinningLogo />
            </div>
            <p className={textClass("muted")}>{t("licence")}</p>
            <p className={textClass("muted")}>{t("copyright")}</p>
          </div>
          <ButtonAnchor
            href={REPO_URL}
            target="_blank"
            rel="noopener"
            aria-label={nav("github")}
            variant="onFrame"
          >
            <GithubLogo size={20} />
          </ButtonAnchor>
        </div>
      </div>
    </footer>
  );
}
