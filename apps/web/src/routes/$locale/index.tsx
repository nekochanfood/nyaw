import { createFileRoute } from "@tanstack/react-router";

import { translate } from "@/i18n/index";
import { GetNyaw } from "@/landing/GetNyaw";
import { Hero } from "@/landing/Hero";
import { Showcase } from "@/landing/Showcase";
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

function Landing() {
  return (
    <>
      <Hero />
      <Showcase />
      <GetNyaw />
    </>
  );
}
