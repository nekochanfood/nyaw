import { createFileRoute } from "@tanstack/react-router";

import { translate } from "@/i18n/index";
import { pageTitle, seo } from "@/seo";

export const Route = createFileRoute("/$locale/features")({
  head: ({ match }) => {
    const { locale, origin } = match.context;
    return seo({
      origin,
      locale,
      path: "/features",
      title: pageTitle(translate(locale, "meta", "features_title")),
      description: translate(locale, "meta", "features_description"),
    });
  },
  component: FeaturesPage,
});

/** 機能のページ。中身はまだない */
function FeaturesPage() {
  return null;
}
