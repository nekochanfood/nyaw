import { ArrowSquareOut } from "@phosphor-icons/react/dist/csr/ArrowSquareOut";
import { CheckCircle } from "@phosphor-icons/react/dist/csr/CheckCircle";
import { DownloadSimple } from "@phosphor-icons/react/dist/csr/DownloadSimple";
import { Flask } from "@phosphor-icons/react/dist/csr/Flask";
import { WarningCircle } from "@phosphor-icons/react/dist/csr/WarningCircle";
import { createFileRoute } from "@tanstack/react-router";
import { type ReactNode, useRef } from "react";

import { megabytes, type Release } from "@/download/releases";
import { useReleases } from "@/download/useReleases";
import type { Locale } from "@/i18n/config";
import { translate, useLocale, useT } from "@/i18n/index";
import { RELEASES_URL } from "@/links";
import { useReveal } from "@/motion/useReveal";
import { pageTitle, seo } from "@/seo";
import { ButtonAnchor } from "@/ui/Button";
import { containerClass, cx, textClass } from "@/ui/variants";

export const Route = createFileRoute("/$locale/download")({
  head: ({ match }) => {
    const { locale, origin } = match.context;
    return seo({
      origin,
      locale,
      path: "/download",
      title: pageTitle(translate(locale, "meta", "download_title")),
      description: translate(locale, "download", "body"),
    });
  },
  component: Download,
});

/** 公開日。英語は日・月・年の順（イギリス英語） */
const formatDate = (date: Date, locale: Locale) =>
  new Intl.DateTimeFormat(locale === "ja" ? "ja-JP" : "en-GB", { dateStyle: "long" }).format(date);

function Card({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div
      data-reveal
      className={cx("flex flex-col gap-4 rounded-xl bg-control/60 p-6 sm:p-8", className)}
    >
      {children}
    </div>
  );
}

type Releases = ReturnType<typeof useReleases>;

function Latest({ state }: { state: Releases }) {
  const t = useT("download");
  const locale = useLocale();
  const stable = state.status === "ready" ? state.stable : undefined;
  if (state.status === "loading") {
    return (
      <output className={cx(textClass("muted"), "block h-12 animate-pulse content-center")}>
        {t("checking")}
      </output>
    );
  }
  if (!stable) {
    return (
      <div className="flex flex-col items-start gap-4">
        <output className="flex items-center gap-2 text-sm text-neutral-text-low">
          <WarningCircle size={18} className="shrink-0 text-ctp-peach" />
          {t("unavailable")}
        </output>
        <ButtonAnchor href={RELEASES_URL} variant="solid" size="lg">
          {t("releases")}
          <ArrowSquareOut size={20} />
        </ButtonAnchor>
      </div>
    );
  }
  return (
    <div className="flex flex-col items-start gap-4">
      <p className="flex flex-wrap items-baseline gap-x-3">
        <span className="text-sm text-neutral-text-low">{t("latest")}</span>
        <span data-testid="version" className="font-serif text-3xl">
          {stable.version}
        </span>
        <span className="text-sm text-neutral-text-low">
          {t("released", { date: formatDate(stable.published, locale) })}
        </span>
      </p>
      <ButtonAnchor href={stable.installer.url} variant="solid" size="lg">
        <DownloadSimple size={20} weight="bold" />
        {t("installer")}
        <span className="font-normal opacity-70">
          {t("size", { size: megabytes(stable.installer.size) })}
        </span>
      </ButtonAnchor>
    </div>
  );
}

function Preview({ state }: { state: Releases }) {
  const t = useT("download");
  const preview: Release | undefined = state.status === "ready" ? state.preview : undefined;
  return (
    <Card>
      <h2 className="flex items-center gap-3 text-xl font-bold">
        <Flask size={22} weight="duotone" className="text-ctp-mauve" aria-hidden />
        {t("preview_title")}
      </h2>
      <p className={textClass("muted")}>{t("preview_body")}</p>
      {preview ? (
        <ButtonAnchor
          href={preview.installer.url}
          variant="surface"
          size="md"
          className="self-start"
        >
          <DownloadSimple size={16} weight="bold" />
          {t("preview_download", { version: preview.version })}
        </ButtonAnchor>
      ) : (
        state.status === "ready" && <p className={textClass("muted")}>{t("preview_none")}</p>
      )}
    </Card>
  );
}

function Download() {
  const t = useT("download");
  const ref = useRef<HTMLDivElement>(null);
  const releases = useReleases();
  useReveal(ref);
  return (
    <div ref={ref} className={cx(containerClass, "flex flex-col gap-16 pt-16 pb-24 sm:pt-24")}>
      <section data-reveal-group className="flex max-w-3xl flex-col gap-6">
        <h1 data-reveal className={textClass("display")}>
          {t("title")}
        </h1>
        <p data-reveal className={textClass("lead")}>
          {t("body")}
        </p>
        <div data-reveal className="pt-2">
          <Latest state={releases} />
        </div>
      </section>
      <div data-reveal-group className="grid gap-3 lg:grid-cols-3">
        <Card>
          <h2 className="text-xl font-bold">{t("requirements_title")}</h2>
          <ul className="flex flex-col gap-3">
            {(["requirement_os", "requirement_runtime", "requirement_headset"] as const).map(
              (key) => (
                <li key={key} className="flex gap-3 text-sm leading-relaxed">
                  <CheckCircle
                    size={20}
                    weight="fill"
                    className="shrink-0 text-ctp-green"
                    aria-hidden
                  />
                  {t(key)}
                </li>
              ),
            )}
          </ul>
        </Card>
        <Card>
          <h2 className="text-xl font-bold">{t("install_title")}</h2>
          <ol className="flex flex-col gap-3">
            {(["install_run", "install_start", "install_setup"] as const).map((key, i) => (
              <li key={key} className="flex gap-3 text-sm leading-relaxed">
                <span
                  aria-hidden
                  className="inline-flex size-6 shrink-0 items-center justify-center rounded-full bg-ctp-lavender/20 text-xs font-bold text-ctp-lavender"
                >
                  {i + 1}
                </span>
                {t(key)}
              </li>
            ))}
          </ol>
          <p className={textClass("muted")}>{t("updates")}</p>
        </Card>
        <Preview state={releases} />
      </div>
      <a
        href={RELEASES_URL}
        target="_blank"
        rel="noopener"
        className="inline-flex items-center gap-2 self-start text-sm text-neutral-text-low hover:text-neutral-text"
      >
        {t("all_releases")}
        <ArrowSquareOut aria-hidden />
      </a>
    </div>
  );
}
