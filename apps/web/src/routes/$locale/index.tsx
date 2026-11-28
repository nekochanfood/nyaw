import { createFileRoute } from "@tanstack/react-router";

import { translate } from "@/i18n/index";
import { IntroLogoSlot } from "@/layout/IntroLogo";
import { useIntroPending } from "@/layout/Loading";
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

/** ランディングページ。今はフルロゴだけを上下左右の中央に置く。最初に開いたときは、読み込み画面のイントロがここへ移る */
function Landing() {
  const pending = useIntroPending();
  return (
    <div className="flex grow items-center justify-center p-(--content-pad-x)">
      <IntroLogoSlot pending={pending} />
    </div>
  );
}
