import { createFileRoute, notFound, Outlet, rootRouteId } from "@tanstack/react-router";

import { isLocale } from "@/i18n/config";
import { NotFound } from "@/layout/NotFound";
import { SiteLayout } from "@/layout/SiteLayout";
import { getOrigin } from "@/seo";

export const Route = createFileRoute("/$locale")({
  beforeLoad: async ({ params }) => {
    // この段階の notFound は、ここのレイアウトを描けないので、ルート（__root）の 404 に任せる
    if (!isLocale(params.locale)) throw notFound({ routeId: rootRouteId });
    // ブラウザでは自分のオリジンが分かるので、サーバーに問い合わせない
    const origin = typeof window === "undefined" ? await getOrigin() : location.origin;
    return { locale: params.locale, origin };
  },
  component: () => (
    <SiteLayout>
      <Outlet />
    </SiteLayout>
  ),
  // 言語の下の存在しないパス。レイアウトの Outlet の場所に出る
  notFoundComponent: NotFound,
});
