import { isLocale, negotiateLocale, switchLocale } from "@/i18n/config";

/**
 * 言語のないページのパス（`/`、`/download`）を、訪問者の言語のパスへ 308 で転送する。
 * ファイル（拡張子のあるパス）と Vite・TanStack Start の内部のパスは対象外。
 */
export function localeRedirect(request: Request): Response | undefined {
  const url = new URL(request.url);
  const first = url.pathname.split("/")[1];
  if (isLocale(first) || /^[_@]/.test(first ?? "") || url.pathname.includes(".")) return;
  const locale = negotiateLocale(
    request.headers.get("cookie"),
    request.headers.get("accept-language"),
  );
  const location = switchLocale(url.pathname, locale) + url.search;
  // Vary がないと、キャッシュが別の言語の訪問者に同じ転送先を返す
  return new Response(null, {
    status: 308,
    headers: { location, vary: "cookie, accept-language" },
  });
}
