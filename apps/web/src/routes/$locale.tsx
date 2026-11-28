import { createFileRoute, notFound, Outlet } from "@tanstack/react-router";
import { useEffect } from "react";

import { isLocale } from "@/i18n/config";
import { Footer } from "@/layout/Footer";
import { Header } from "@/layout/Header";
import { PageTransition } from "@/layout/PageTransition";
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
      {/* ヘッダーは上辺の帯に載せる。本文のカードの外に置き、フッターと同じく帯の色の上に並ぶ */}
      <Header />
      {/*
        帯の内側に載る本文のカード。下端だけを丸め、その先の帯の色にフッターが続く。上端の角は、画面に固定した
        帯（__root）が丸める。上下の端には、帯にかかる本文をぼかす層（styles.css の .edge-fade）を置く。
        clip は hidden と違いスクロールの箱を作らないので、ぼかしの層の sticky が効いたまま角で切り取れる。
        角はここだけ通常の角丸にし、半径も倍率を掛ける前の値にする。Chromium では、スーパーエリプスの角で
        切り取った箱の内側にあると、ぼかしの層（backdrop-filter と mask-image）のマスクが効かなくなり、
        層が不透明のパネルになる
      */}
      <div className="frame-x flex min-h-[calc(100svh-var(--frame-top)-var(--frame-gutter))] flex-col overflow-clip rounded-b-[calc(var(--frame-radius)/var(--corner-scale))] bg-card [corner-shape:round]">
        <div aria-hidden className="edge-fade edge-fade-top" />
        <main id="main" className="flex grow flex-col">
          <PageTransition>
            <Outlet />
          </PageTransition>
        </main>
        <div aria-hidden className="edge-fade edge-fade-bottom" />
      </div>
      <Footer />
    </TooltipProvider>
  );
}
