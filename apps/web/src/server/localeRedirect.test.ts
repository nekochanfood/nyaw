import { describe, expect, it } from "vitest";

import { localeRedirect } from "@/server/localeRedirect";

const get = (path: string, headers: Record<string, string> = {}) =>
  localeRedirect(new Request(`https://nyaw.example${path}`, { headers }));

describe("localeRedirect", () => {
  it("ルートを訪問者の言語へ転送する", () => {
    const res = get("/?ref=x", { "accept-language": "ja-JP,ja;q=0.9" });
    expect(res?.status).toBe(308);
    expect(res?.headers.get("location")).toBe("/ja?ref=x");
    expect(res?.headers.get("vary")).toContain("accept-language");
  });

  it("言語のないページも転送する", () => {
    expect(get("/download", { cookie: "locale=ja" })?.headers.get("location")).toBe("/ja/download");
  });

  it("言語のあるページ・ファイル・内部のパスは転送しない", () => {
    for (const path of [
      "/en",
      "/ja/download",
      "/scene/dark-en/x.png",
      "/_serverFn/abc",
      "/@vite/client",
    ]) {
      expect(get(path), path).toBeUndefined();
    }
  });
});
