import { Menu } from "@base-ui/react/menu";
import { Check } from "@phosphor-icons/react/dist/csr/Check";
import { Translate } from "@phosphor-icons/react/dist/csr/Translate";
import { useLocation, useNavigate } from "@tanstack/react-router";
import { useState } from "react";

import { isLocale, LOCALE_COOKIE, type Locale, locales, switchLocale } from "@/i18n/config";
import { useLocale, useT } from "@/i18n/index";
import { MenuPopup, Tooltip } from "@/ui/Popup";
import { buttonClass, menuItemClass } from "@/ui/variants";

const NAMES: Record<Locale, string> = { en: "English", ja: "日本語" };

/** 言語を切り替え、次にルート（`/`）を開いたときのために cookie に残す */
export function useChangeLocale() {
  const navigate = useNavigate();
  const { pathname, hash } = useLocation();
  return (locale: string) => {
    if (!isLocale(locale)) return;
    document.cookie = `${LOCALE_COOKIE}=${locale}; path=/; max-age=31536000; samesite=lax`;
    // 同じページの言語を替えるだけなので、スクロールの位置は保つ
    void navigate({
      href: switchLocale(pathname, locale) + (hash ? `#${hash}` : ""),
      resetScroll: false,
    });
  };
}

export function LanguageMenu() {
  const t = useT("nav");
  const locale = useLocale();
  const changeLocale = useChangeLocale();
  const [open, setOpen] = useState(false);
  return (
    <Menu.Root open={open} onOpenChange={setOpen} modal={false}>
      <Tooltip label={t("language")}>
        <Menu.Trigger aria-label={t("language")} className={buttonClass({ variant: "onFrame" })}>
          <Translate size={20} />
        </Menu.Trigger>
      </Tooltip>
      <MenuPopup open={open} className="w-44">
        <Menu.RadioGroup value={locale} onValueChange={changeLocale}>
          {locales.map((l) => (
            <Menu.RadioItem key={l} value={l} closeOnClick className={menuItemClass} lang={l}>
              <span className="grow">{NAMES[l]}</span>
              <Menu.RadioItemIndicator>
                <Check size={16} weight="bold" />
              </Menu.RadioItemIndicator>
            </Menu.RadioItem>
          ))}
        </Menu.RadioGroup>
      </MenuPopup>
    </Menu.Root>
  );
}
