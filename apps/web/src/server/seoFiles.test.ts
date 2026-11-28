import { describe, expect, it } from "vitest";

import { seoFile } from "@/server/seoFiles";

const get = (path: string) => seoFile(new Request(`https://nyaw.example${path}`));

describe("seoFile", () => {
  it("robots.txt はサイトマップの場所を自分のオリジンで示す", async () => {
    const res = get("/robots.txt");
    expect(res?.headers.get("content-type")).toContain("text/plain");
    expect(await res?.text()).toContain("Sitemap: https://nyaw.example/sitemap.xml");
  });

  it("sitemap.xml は各ページを言語ごとに載せ、別の言語の版を示す", async () => {
    const xml = (await get("/sitemap.xml")?.text()) ?? "";
    for (const loc of ["/en", "/ja", "/en/features", "/ja/download"]) {
      expect(xml).toContain(`<loc>https://nyaw.example${loc}</loc>`);
    }
    expect(xml).toContain('hreflang="ja" href="https://nyaw.example/ja/download"');
    expect(xml.match(/<url>/g)).toHaveLength(6);
  });

  it("ほかのパスには答えない", () => {
    expect(get("/en")).toBeUndefined();
    expect(get("/sitemap.xml.bak")).toBeUndefined();
  });
});
