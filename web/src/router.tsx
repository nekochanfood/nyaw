import {
  createRootRoute,
  createRoute,
  createRouter,
  lazyRouteComponent,
  Outlet,
} from "@tanstack/react-router";

const Testing = lazyRouteComponent(() => import("./testing/App"), "App");
const TestingTransparent = lazyRouteComponent(() => import("./testing-transparent/App"), "App");
const PointerTest = lazyRouteComponent(() => import("./testing-pointer/App"), "App");

const rootRoute = createRootRoute({
  component: Outlet,
  notFoundComponent: () => <p>ページが見つかりません</p>,
});
// ディレクトリの URL と、Rust が開く HTML の URL の両方で同じ画面を使う。
const testingRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/testing",
  component: Testing,
});
const testingEntryRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/testing/index.html",
  component: Testing,
});
const transparentRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/testing-transparent",
  component: TestingTransparent,
});
const transparentEntryRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/testing-transparent/index.html",
  component: TestingTransparent,
});
const pointerRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/testing-pointer",
  component: PointerTest,
});
const pointerEntryRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/testing-pointer/index.html",
  component: PointerTest,
});

export const router = createRouter({
  routeTree: rootRoute.addChildren([
    testingRoute,
    testingEntryRoute,
    transparentRoute,
    transparentEntryRoute,
    pointerRoute,
    pointerEntryRoute,
  ]),
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
