import { expect, test } from "@playwright/test";

import { gotoStill, watchErrors } from "./helpers";

test.describe("言語の振り分け", () => {
  test("ルートは Accept-Language の言語へ転送する", async ({ request }) => {
    const res = await request.get("/", {
      headers: { "Accept-Language": "ja-JP,ja;q=0.9" },
      maxRedirects: 0,
    });
    expect(res.status()).toBe(308);
    expect(res.headers().location).toBe("/ja");
  });

  test("cookie の言語を Accept-Language より優先する", async ({ request }) => {
    const res = await request.get("/download", {
      headers: { "Accept-Language": "ja", Cookie: "locale=en" },
      maxRedirects: 0,
    });
    expect(res.headers().location).toBe("/en/download");
  });

  test("対応していない言語のパスは 404 のページを出す", async ({ page }) => {
    await gotoStill(page, "/fr");
    await expect(page.getByRole("heading", { name: "Page not found" })).toBeVisible();
  });
});

for (const path of ["/en", "/ja", "/en/features", "/ja/download"]) {
  test(`${path} がエラーなく開く`, async ({ page }) => {
    const assertNoErrors = watchErrors(page);
    await gotoStill(page, path);
    await expect(page.locator("html")).toHaveAttribute("lang", path.slice(1, 3));
    await expect(page.getByRole("banner")).toBeVisible();
    await expect(page.getByRole("contentinfo")).toBeVisible();
    assertNoErrors();
  });
}

test("head に SEO と OGP のタグがある", async ({ page, baseURL }) => {
  await gotoStill(page, "/ja/download");
  await expect(page).toHaveTitle("ダウンロード / nyaw");
  await expect(page.locator('link[rel="canonical"]')).toHaveAttribute(
    "href",
    `${baseURL}/ja/download`,
  );
  await expect(page.locator('link[hreflang="en"]')).toHaveAttribute(
    "href",
    `${baseURL}/en/download`,
  );
});

test("robots.txt と sitemap.xml を返す", async ({ request, baseURL }) => {
  expect(await (await request.get("/robots.txt")).text()).toContain(
    `Sitemap: ${baseURL}/sitemap.xml`,
  );
  expect(await (await request.get("/sitemap.xml")).text()).toContain(
    `<loc>${baseURL}/ja/features</loc>`,
  );
});
