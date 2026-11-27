import { createFileRoute } from "@tanstack/react-router";
import { useRef } from "react";

import { translate, useT } from "@/i18n/index";
import { Features } from "@/landing/Features";
import { GetNyaw } from "@/landing/GetNyaw";
import { PageSection, PageTitle } from "@/layout/Page";
import { useReveal } from "@/motion/useReveal";
import { pageTitle, seo } from "@/seo";
import { textClass } from "@/ui/variants";

export const Route = createFileRoute("/$locale/features")({
  head: ({ match }) => {
    const { locale, origin } = match.context;
    return seo({
      origin,
      locale,
      path: "/features",
      title: pageTitle(translate(locale, "meta", "features_title")),
      description: translate(locale, "features", "body"),
    });
  },
  component: FeaturesPage,
});

function FeaturesPage() {
  const t = useT("features");
  const ref = useRef<HTMLDivElement>(null);
  useReveal(ref);
  return (
    <>
      <PageTitle title={t("title")} />
      <PageSection>
        <div ref={ref} className="flex flex-col gap-12">
          <div data-reveal-group>
            <p data-reveal className={textClass("lead", "max-w-2xl")}>
              {t("body")}
            </p>
          </div>
          <Features />
        </div>
      </PageSection>
      <div className="bg-background">
        <GetNyaw />
      </div>
    </>
  );
}
