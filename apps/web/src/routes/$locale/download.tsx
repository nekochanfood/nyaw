import { ArrowSquareOut } from "@phosphor-icons/react/dist/csr/ArrowSquareOut";
import { CheckCircle } from "@phosphor-icons/react/dist/csr/CheckCircle";
import { createFileRoute } from "@tanstack/react-router";
import { type ReactNode, useRef } from "react";

import { ChannelPicker } from "@/download/ChannelPicker";
import { useReleases } from "@/download/useReleases";
import { translate, useT } from "@/i18n/index";
import { PageSection, PageTitle } from "@/layout/Page";
import { RELEASES_URL } from "@/links";
import { useReveal } from "@/motion/useReveal";
import { pageTitle, seo } from "@/seo";
import { cx, textClass } from "@/ui/variants";

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

function Card({
  title,
  children,
  className,
}: {
  title?: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      data-reveal
      className={cx("flex flex-col gap-4 rounded-2xl bg-card p-6 sm:p-8", className)}
    >
      {title && <h2 className="text-xl font-bold">{title}</h2>}
      {children}
    </div>
  );
}

function Download() {
  const t = useT("download");
  const nav = useT("nav");
  const ref = useRef<HTMLDivElement>(null);
  const releases = useReleases();
  useReveal(ref);
  return (
    <>
      <PageTitle title={nav("download")} />
      <PageSection>
        <div ref={ref} className="flex flex-col gap-12">
          <div data-reveal-group>
            <p data-reveal className={textClass("lead", "max-w-2xl")}>
              {t("body")}
            </p>
          </div>
          <div data-reveal-group>
            <Card>
              <ChannelPicker state={releases} />
            </Card>
          </div>
          <div data-reveal-group className="grid gap-3 md:grid-cols-2">
            <Card title={t("requirements_title")}>
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
            <Card title={t("install_title")}>
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
          </div>
          <a
            href={RELEASES_URL}
            target="_blank"
            rel="noopener"
            className="inline-flex items-center gap-2 self-start rounded-full text-sm text-neutral-text-low transition-colors hover:text-neutral-text"
          >
            {t("all_releases")}
            <ArrowSquareOut aria-hidden />
          </a>
        </div>
      </PageSection>
    </>
  );
}
