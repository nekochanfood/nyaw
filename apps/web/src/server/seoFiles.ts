import { locales } from "@/i18n/config";

/** 言語を除いたページのパス。sitemap.xml に載せる */
export const PAGES = ["/", "/features", "/download"] as const;

const url = (origin: string, locale: string, page: string) =>
  `${origin}/${locale}${page === "/" ? "" : page}`;

export function robotsTxt(origin: string) {
  return `User-agent: *\nAllow: /\n\nSitemap: ${origin}/sitemap.xml\n`;
}

/** 言語ごとのページと、その別の言語の版（hreflang）を並べる */
export function sitemapXml(origin: string) {
  const entries = PAGES.flatMap((page) =>
    locales.map((locale) => {
      const alternates = locales
        .map(
          (l) => `    <xhtml:link rel="alternate" hreflang="${l}" href="${url(origin, l, page)}"/>`,
        )
        .join("\n");
      return `  <url>\n    <loc>${url(origin, locale, page)}</loc>\n${alternates}\n  </url>`;
    }),
  );
  return [
    `<?xml version="1.0" encoding="UTF-8"?>`,
    `<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">`,
    ...entries,
    `</urlset>`,
    "",
  ].join("\n");
}

/** robots.txt と sitemap.xml のリクエストに答える。それ以外は `undefined` */
export function seoFile(request: Request): Response | undefined {
  const { origin, pathname } = new URL(request.url);
  if (pathname === "/robots.txt") {
    return new Response(robotsTxt(origin), {
      headers: { "content-type": "text/plain; charset=utf-8" },
    });
  }
  if (pathname === "/sitemap.xml") {
    return new Response(sitemapXml(origin), {
      headers: { "content-type": "application/xml; charset=utf-8" },
    });
  }
}
