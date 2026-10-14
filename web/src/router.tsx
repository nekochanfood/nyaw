import { createRootRoute, createRoute, createRouter, Outlet } from "@tanstack/react-router";
import { App as Testing } from "./testing/App";
import { App as TestingTransparent } from "./testing-transparent/App";

const rootRoute = createRootRoute({
  component: Outlet,
  notFoundComponent: () => <p>ページが見つかりません</p>,
});
// ディレクトリの URL と、Rust が開く HTML の URL の両方で同じ画面を使う。
const testingRoute = createRoute({ getParentRoute: () => rootRoute, path: "/testing", component: Testing });
const testingEntryRoute = createRoute({ getParentRoute: () => rootRoute, path: "/testing/index.html", component: Testing });
const transparentRoute = createRoute({ getParentRoute: () => rootRoute, path: "/testing-transparent", component: TestingTransparent });
const transparentEntryRoute = createRoute({ getParentRoute: () => rootRoute, path: "/testing-transparent/index.html", component: TestingTransparent });

export const router = createRouter({
  routeTree: rootRoute.addChildren([testingRoute, testingEntryRoute, transparentRoute, transparentEntryRoute]),
});

declare module "@tanstack/react-router" {
  interface Register { router: typeof router; }
}
