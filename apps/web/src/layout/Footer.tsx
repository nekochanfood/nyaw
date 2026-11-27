import { ArrowSquareOut } from "@phosphor-icons/react/dist/csr/ArrowSquareOut";
import { GithubLogo } from "@phosphor-icons/react/dist/csr/GithubLogo";
import { Link } from "@tanstack/react-router";
import type { ReactNode } from "react";

import { useLocale, useT } from "@/i18n/index";
import { ISSUES_URL, RELEASES_URL, REPO_URL } from "@/links";
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
    <div className="flex min-w-36 flex-col gap-1.5">
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

/** 帯と同じ色の面に置くフッター。本文のカードの下に続く */
export function Footer() {
  const t = useT("footer");
  const nav = useT("nav");
  const locale = useLocale();
  return (
    <footer className="px-(--frame-gutter) pt-14 pb-12">
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
          <Logo className="h-20 w-auto self-start sm:h-28" />
        </div>
        <div className="flex items-end justify-between gap-6">
          <div className="flex flex-col gap-1">
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
