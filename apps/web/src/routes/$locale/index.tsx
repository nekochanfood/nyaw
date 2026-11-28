import { createFileRoute } from "@tanstack/react-router";

import { translate } from "@/i18n/index";
import { pageTitle, seo } from "@/seo";

export const Route = createFileRoute("/$locale/")({
  head: ({ match }) => {
    const { locale, origin } = match.context;
    return seo({
      origin,
      locale,
      path: "/",
      title: pageTitle(translate(locale, "meta", "title")),
      description: translate(locale, "meta", "description"),
    });
  },
  component: Landing,
});

/** ランディングページ。中身はまだない */
function Landing() {
  return null;
}
