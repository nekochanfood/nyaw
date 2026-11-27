import { createFileRoute, notFound, Outlet } from "@tanstack/react-router";
import { useEffect } from "react";

import { isLocale } from "@/i18n/config";
import { Footer } from "@/layout/Footer";
import { Header } from "@/layout/Header";
import { useSmoothScroll } from "@/motion/useSmoothScroll";
import { getOrigin } from "@/seo";
import { useLocale, useT } from "@/i18n/index";
import { TooltipProvider } from "@/ui/Popup";

export const Route = createFileRoute("/$locale")({
  beforeLoad: async ({ params }) => {
    if (!isLocale(params.locale)) throw notFound();
    // ブラウザでは自分のオリジンが分かるので、サーバーに問い合わせない
    const origin = typeof window === "undefined" ? await getOrigin() : location.origin;
    return { locale: params.locale, origin };
  },
  component: Layout,
});

function Layout() {
  const t = useT("nav");
  const locale = useLocale();
  useSmoothScroll();
  // ページを移っても <html> は描き直されないので、言語を切り替えたら合わせる
  useEffect(() => {
    document.documentElement.lang = locale;
  }, [locale]);
  return (
    <TooltipProvider delay={400}>
      <a
        href="#main"
        className="fixed top-4 left-4 z-50 -translate-y-24 rounded-full bg-neutral-text px-4 py-2 text-sm text-card focus:translate-y-0"
      >
        {t("skip")}
      </a>
      {/* 帯の内側に載る本文のカード。下端だけを丸め、その先の帯の色にフッターが続く */}
      <div className="m-(--frame-gutter) mb-0 flex min-h-[calc(100svh-2*var(--frame-gutter))] flex-col rounded-b-(--frame-radius) bg-card">
        <Header />
        <main id="main" className="grow">
          <Outlet />
        </main>
      </div>
      <Footer />
    </TooltipProvider>
  );
}
