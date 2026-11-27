import { Link } from "@tanstack/react-router";

import { translate, useLocale, useT } from "@/i18n/index";
import { pageTitle } from "@/seo";
import { buttonClass, textClass } from "@/ui/variants";

export function NotFound() {
  const t = useT("not_found");
  const locale = useLocale();
  return (
    <div className="m-(--frame-gutter) flex min-h-[calc(100svh-2*var(--frame-gutter))] flex-col items-center justify-center gap-6 bg-card p-8 text-center">
      <title>{pageTitle(translate(locale, "meta", "not_found_title"))}</title>
      <meta name="robots" content="noindex" />
      <p className="font-serif text-7xl text-neutral-text-low">404</p>
      <h1 className={textClass("section")}>{t("title")}</h1>
      <Link
        to="/$locale"
        params={{ locale }}
        className={buttonClass({ variant: "solid", size: "md" })}
      >
        {t("home")}
      </Link>
    </div>
  );
}
