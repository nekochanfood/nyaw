/// <reference types="vite/client" />
import { createRootRoute, HeadContent, Outlet, Scripts, useParams } from "@tanstack/react-router";
import { type ReactNode, useEffect } from "react";

import iconDark from "@assets/logos/Logo_Mini_Dark.svg?url";
import iconLight from "@assets/logos/Logo_Mini_Light.svg?url";
import { defaultLocale, isLocale } from "@/i18n/config";
import appCss from "@/styles.css?url";
import { themeScript } from "@/theme";
import { NotFound } from "@/layout/NotFound";

export const Route = createRootRoute({
  head: () => ({
    meta: [
      { charSet: "utf-8" },
      { name: "viewport", content: "width=device-width, initial-scale=1, viewport-fit=cover" },
    ],
    links: [
      { rel: "stylesheet", href: appCss },
      {
        rel: "icon",
        href: iconLight,
        type: "image/svg+xml",
        media: "(prefers-color-scheme: light)",
      },
      { rel: "icon", href: iconDark, type: "image/svg+xml", media: "(prefers-color-scheme: dark)" },
    ],
  }),
  shellComponent: Shell,
  component: Outlet,
  notFoundComponent: NotFound,
});

function Shell({ children }: { children: ReactNode }) {
  const { locale } = useParams({ strict: false });
  // 操作を受け付けられるようになった印。E2E テストはこれを待ってから操作する
  useEffect(() => {
    document.documentElement.dataset.hydrated = "";
  }, []);
  return (
    // data-theme は描画の前に themeScript が付けるので、サーバーの HTML と食い違う
    <html lang={isLocale(locale) ? locale : defaultLocale} suppressHydrationWarning>
      <head>
        {/* oxlint-disable-next-line react/no-danger -- 保存したテーマを描画の前に反映する。中身は固定の文字列 */}
        <script dangerouslySetInnerHTML={{ __html: themeScript }} />
        <HeadContent />
      </head>
      <body>
        {/* Base UI のポップアップを前面に出すため、ページの重なり順を分ける */}
        <div className="isolate">{children}</div>
        {/* 画面の四辺を帯で覆い、内側の角を丸める。スクロールしても動かない */}
        <div
          aria-hidden
          className="pointer-events-none fixed inset-(--frame-gutter) z-40 rounded-(--frame-radius) shadow-[0_0_0_100vmax_var(--groove)] transition-shadow duration-300"
        />
        <Scripts />
      </body>
    </html>
  );
}
