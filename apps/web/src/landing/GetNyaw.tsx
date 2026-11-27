import { ArrowRight } from "@phosphor-icons/react/dist/csr/ArrowRight";
import { useRef } from "react";

import { useLocale, useT } from "@/i18n/index";
import { Logo } from "@/layout/Logo";
import { useReveal } from "@/motion/useReveal";
import { ButtonLink } from "@/ui/Button";
import { containerClass, cx, textClass } from "@/ui/variants";

/** ページの終わりの、ダウンロードへの案内 */
export function GetNyaw() {
  const t = useT("cta");
  const locale = useLocale();
  const ref = useRef<HTMLElement>(null);
  useReveal(ref);
  return (
    <section ref={ref} className={cx(containerClass, "pb-24 sm:pb-32")}>
      <div
        data-reveal-group
        className="flex flex-col items-start gap-6 rounded-2xl bg-groove/50 p-8 sm:flex-row sm:items-center sm:justify-between sm:p-12"
      >
        <div className="flex flex-col gap-3">
          <div data-reveal>
            <Logo className="h-10 w-auto" />
          </div>
          <h2 data-reveal className={textClass("section")}>
            {t("title")}
          </h2>
          <p data-reveal className={textClass("lead")}>
            {t("body")}
          </p>
        </div>
        <div data-reveal>
          <ButtonLink to="/$locale/download" params={{ locale }} variant="solid" size="lg">
            {t("download")}
            <ArrowRight size={20} weight="bold" />
          </ButtonLink>
        </div>
      </div>
    </section>
  );
}
