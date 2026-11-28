import { Link } from "@tanstack/react-router";

import { translate, useLocale, useT } from "@/i18n/index";
import { SquareStage } from "@/layout/SquareStage";
import { pageTitle } from "@/seo";
import { buttonClass, textClass } from "@/ui/variants";

export function NotFound() {
  const t = useT("not_found");
  const locale = useLocale();
  return (
    <SquareStage className="flex flex-col items-center justify-center gap-6 p-8 text-center">
      <title>{pageTitle(translate(locale, "meta", "not_found_title"))}</title>
      <meta name="robots" content="noindex" />
      <p className="text-7xl font-bold text-neutral-text-low">404</p>
      <h1 className={textClass("section")}>{t("title")}</h1>
      <Link
        to="/$locale"
        params={{ locale }}
        className={buttonClass({ variant: "solid", size: "md" })}
      >
        {t("home")}
      </Link>
    </SquareStage>
  );
}
