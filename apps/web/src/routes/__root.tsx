/// <reference types="vite/client" />
import { createRootRoute, HeadContent, Outlet, Scripts } from "@tanstack/react-router";
import appCss from "../styles.css?url";

export const Route = createRootRoute({
  head: () => ({
    meta: [
      { charSet: "utf-8" },
      { name: "viewport", content: "width=device-width, initial-scale=1" },
      { title: "nyaw" },
    ],
    links: [{ rel: "stylesheet", href: appCss }],
  }),
  component: Root,
});

function Root() {
  return (
    <html lang="ja">
      <head>
        <HeadContent />
      </head>
      <body>
        {/* Base UI のポップアップを前面に出すため、アプリの重なり順を分ける */}
        <div className="isolate">
          <Outlet />
        </div>
        <Scripts />
      </body>
    </html>
  );
}
