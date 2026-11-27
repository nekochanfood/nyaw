import { useParams } from "@tanstack/react-router";

import en from "@locale/en.json";
import ja from "@locale/ja.json";
import { defaultLocale, isLocale, type Locale } from "@/i18n/config";

export type Messages = typeof en;
export type Namespace = keyof Messages;

export const messages: Record<Locale, Messages> = { en, ja };

/** `{name}` を `vars` の値で置き換える。訳がなければ英語、英語にもなければキーを返す */
export function translate<N extends Namespace>(
  locale: Locale,
  ns: N,
  key: keyof Messages[N] & string,
  vars: Record<string, string | number> = {},
) {
  const text: string =
    (messages[locale][ns] as Record<string, string>)[key] ??
    (messages.en[ns] as Record<string, string>)[key] ??
    key;
  return text.replace(/\{(\w+)\}/g, (all, name: string) => String(vars[name] ?? all));
}

/** URL の先頭の言語。言語のないページ（404 など）は既定の言語 */
export function useLocale(): Locale {
  const { locale } = useParams({ strict: false });
  return isLocale(locale) ? locale : defaultLocale;
}

export function useT<N extends Namespace>(ns: N) {
  const locale = useLocale();
  return (key: keyof Messages[N] & string, vars?: Record<string, string | number>) =>
    translate(locale, ns, key, vars);
}
