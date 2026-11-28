import { useLocale, useT } from "@/i18n/index";

/** ヘッダーとモバイルのメニューに並べるページ */
export function useNavItems() {
  const t = useT("nav");
  const locale = useLocale();
  return [
    { label: t("features"), to: "/$locale/features", hash: undefined, params: { locale } },
    { label: t("download"), to: "/$locale/download", hash: undefined, params: { locale } },
  ] as const;
}
