import { createRootRoute, createRoute, createRouter, Outlet } from "@tanstack/react-router";
import { Testing } from "../../crates/webtesting/web/Testing";
import { Laser } from "../../crates/webbeam/web/Laser";
import { Cursor } from "../../crates/webbeam/web/Cursor";

const rootRoute = createRootRoute({
  component: Outlet,
  notFoundComponent: () => <p>ページが見つかりません</p>,
});
const testingRoute = createRoute({ getParentRoute: () => rootRoute, path: "/webtesting", component: Testing });
const laserRoute = createRoute({ getParentRoute: () => rootRoute, path: "/laser", component: Laser });
const cursorRoute = createRoute({ getParentRoute: () => rootRoute, path: "/cursor", component: Cursor });

export const router = createRouter({
  routeTree: rootRoute.addChildren([testingRoute, laserRoute, cursorRoute]),
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
