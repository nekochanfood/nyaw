export const locales = ["en", "ja"] as const;

export type Locale = (typeof locales)[number];

export const defaultLocale: Locale = "en";

/** 言語メニューで選んだ言語。次にルート（`/`）を開いたときの振り分けに使う */
export const LOCALE_COOKIE = "locale";

export function isLocale(value: string | undefined | null): value is Locale {
  return !!value && (locales as readonly string[]).includes(value);
}

/** cookie、次に Accept-Language の順に、対応している言語を選ぶ */
export function negotiateLocale(
  cookieHeader: string | null,
  acceptLanguage: string | null,
): Locale {
  const cookie = cookieHeader
    ?.split(";")
    .map((part) => part.trim())
    .find((part) => part.startsWith(`${LOCALE_COOKIE}=`))
    ?.slice(LOCALE_COOKIE.length + 1);
  if (isLocale(cookie)) return cookie;
  // q 値は見ない。ブラウザは優先度の高い順に並べて送る
  for (const tag of (acceptLanguage ?? "").split(",")) {
    const lang = tag.split(";")[0]?.trim().toLowerCase().split("-")[0];
    if (isLocale(lang)) return lang;
  }
  return defaultLocale;
}

/** `/ja/download` の言語の部分を `locale` に替える。言語のないパスには前に付ける */
export function switchLocale(pathname: string, locale: Locale) {
  const [, first, ...rest] = pathname.split("/");
  const tail = isLocale(first) ? rest : [first, ...rest];
  return `/${[locale, ...tail].filter(Boolean).join("/")}`;
}
