import { createFileRoute } from "@tanstack/react-router";

import { translate } from "@/i18n/index";
import { pageTitle, seo } from "@/seo";

export const Route = createFileRoute("/$locale/download")({
  head: ({ match }) => {
    const { locale, origin } = match.context;
    return seo({
      origin,
      locale,
      path: "/download",
      title: pageTitle(translate(locale, "meta", "download_title")),
      description: translate(locale, "meta", "download_description"),
    });
  },
  component: Download,
});

/** ダウンロードのページ。中身はまだない */
function Download() {
  return null;
}
