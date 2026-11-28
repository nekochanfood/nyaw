import { createServerFn } from "@tanstack/react-start";
import { getRequest } from "@tanstack/react-start/server";

import { type Locale, locales } from "@/i18n/config";

/** 配信しているオリジン。canonical や OGP の画像は絶対 URL で書く */
export const getOrigin = createServerFn().handler(() => new URL(getRequest().url).origin);

/** ページのタイトル。「（ページ名） / nyaw」 */
export const pageTitle = (page: string) => `${page} / nyaw`;

const OG_LOCALE: Record<Locale, string> = { en: "en_GB", ja: "ja_JP" };

/** ページの head。`path` は言語を除いたパス（`/`、`/download`） */
export function seo({
  origin,
  locale,
  path,
  title,
  description,
}: {
  origin: string;
  locale: Locale;
  path: string;
  title: string;
  description: string;
}) {
  const url = (l: Locale) => `${origin}/${l}${path === "/" ? "" : path}`;
  return {
    meta: [
      { title },
      { name: "description", content: description },
      { property: "og:type", content: "website" },
      { property: "og:site_name", content: "nyaw" },
      { property: "og:title", content: title },
      { property: "og:description", content: description },
      { property: "og:url", content: url(locale) },
      { property: "og:locale", content: OG_LOCALE[locale] },
      // OGP の画像はまだない。用意したら og:image と twitter:image を足し、カードを summary_large_image にする
      { name: "twitter:card", content: "summary" },
      { name: "twitter:title", content: title },
      { name: "twitter:description", content: description },
    ],
    links: [
      { rel: "canonical", href: url(locale) },
      ...locales.map((l) => ({ rel: "alternate", hrefLang: l, href: url(l) })),
      { rel: "alternate", hrefLang: "x-default", href: `${origin}${path}` },
    ],
  };
}
