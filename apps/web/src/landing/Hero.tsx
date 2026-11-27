import { DownloadSimple } from "@phosphor-icons/react/dist/csr/DownloadSimple";
import { GithubLogo } from "@phosphor-icons/react/dist/csr/GithubLogo";
import { useRef } from "react";

import { useLocale, useT } from "@/i18n/index";
import { REPO_URL } from "@/links";
import { useReveal } from "@/motion/useReveal";
import { ButtonAnchor, ButtonLink } from "@/ui/Button";
import { containerClass, cx, textClass } from "@/ui/variants";

export function Hero() {
  const t = useT("hero");
  const locale = useLocale();
  const ref = useRef<HTMLElement>(null);
  useReveal(ref);
  return (
    <section ref={ref} className={cx(containerClass, "pt-16 pb-12 sm:pt-24 lg:pt-32")}>
      <div data-reveal-group className="flex max-w-3xl flex-col items-start gap-6">
        <p
          data-reveal
          className="flex items-center gap-2 text-sm font-medium text-neutral-text-low"
        >
          <span aria-hidden className="size-2 rounded-full bg-ctp-lavender" />
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
    </section>
  );
}
