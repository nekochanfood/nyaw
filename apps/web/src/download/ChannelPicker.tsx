import { Tabs } from "@base-ui/react/tabs";
import { ArrowSquareOut } from "@phosphor-icons/react/dist/csr/ArrowSquareOut";
import { DownloadSimple } from "@phosphor-icons/react/dist/csr/DownloadSimple";
import { Flask } from "@phosphor-icons/react/dist/csr/Flask";
import { ShieldCheck } from "@phosphor-icons/react/dist/csr/ShieldCheck";
import { WarningCircle } from "@phosphor-icons/react/dist/csr/WarningCircle";
import { animate } from "animejs";
import { useCallback, useState } from "react";

import { megabytes, type Release } from "@/download/releases";
import type { useReleases } from "@/download/useReleases";
import type { Locale } from "@/i18n/config";
import { useLocale, useT } from "@/i18n/index";
import { RELEASES_URL } from "@/links";
import { reducedMotion } from "@/motion/reducedMotion";
import { ButtonAnchor } from "@/ui/Button";
import { cx, textClass } from "@/ui/variants";

type Channel = "stable" | "nightly";

const CHANNELS = {
  stable: { icon: ShieldCheck, tint: "text-ctp-green" },
  nightly: { icon: Flask, tint: "text-ctp-mauve" },
} as const;

/** 公開日。英語は日・月・年の順（イギリス英語） */
const formatDate = (date: Date, locale: Locale) =>
  new Intl.DateTimeFormat(locale === "ja" ? "ja-JP" : "en-GB", { dateStyle: "long" }).format(date);

/** 選んだ版の番号、公開日、インストーラーのボタン */
function Build({ release }: { release: Release }) {
  const t = useT("download");
  const locale = useLocale();
  return (
    <div className="flex flex-col items-start gap-4">
      <p className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
        <span data-testid="version" className="text-3xl font-bold tabular-nums">
          {release.version}
        </span>
        <span className="text-sm text-neutral-text-low">
          {t("released", { date: formatDate(release.published, locale) })}
        </span>
      </p>
      <ButtonAnchor href={release.installer.url} variant="solid" size="lg">
        <DownloadSimple size={20} weight="bold" />
        {t("installer")}
        <span className="font-normal opacity-70">
          {t("size", { size: megabytes(release.installer.size) })}
        </span>
      </ButtonAnchor>
    </div>
  );
}

/** 取得できなかったときに、GitHub Releases へ案内する */
function Unavailable() {
  const t = useT("download");
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

/** 安定版とナイトリーを切り替えて、それぞれの最新版をダウンロードする */
export function ChannelPicker({ state }: { state: ReturnType<typeof useReleases> }) {
  const t = useT("download");
  const [channel, setChannel] = useState<Channel>("stable");
  // 切り替えたパネルは、下から少し浮かび上がる
  const enter = useCallback((el: HTMLDivElement | null) => {
    if (!el || reducedMotion()) return;
    animate(el, { opacity: [0, 1], y: [8, 0], duration: 250, ease: "out(3)" });
  }, []);

  const body = (c: Channel) => {
    if (state.status === "loading") {
      return (
        <output className={cx(textClass("muted"), "block h-12 animate-pulse content-center")}>
          {t("checking")}
        </output>
      );
    }
    if (state.status === "error") return <Unavailable />;
    const release = c === "stable" ? state.stable : state.nightly;
    if (release) return <Build release={release} />;
    return c === "nightly" ? (
      <p className={textClass("muted")}>{t("nightly_none")}</p>
    ) : (
      <Unavailable />
    );
  };

  return (
    <Tabs.Root
      value={channel}
      onValueChange={(v: Channel) => setChannel(v)}
      className="flex flex-col gap-6"
    >
      <Tabs.List
        aria-label={t("channel")}
        className="relative z-0 inline-flex self-start rounded-full bg-control p-1"
      >
        {(Object.keys(CHANNELS) as Channel[]).map((c) => {
          const { icon: Icon, tint } = CHANNELS[c];
          return (
            <Tabs.Tab
              key={c}
              value={c}
              className="flex h-9 cursor-pointer items-center gap-2 rounded-full px-4 text-sm font-medium text-neutral-text-low transition-colors duration-(--duration-normal) outline-none hover:text-neutral-text focus-visible:outline-2 data-active:text-neutral-text"
            >
              <Icon size={18} weight="fill" className={tint} aria-hidden />
              {t(c)}
            </Tabs.Tab>
          );
        })}
        {/* 選んだタブの下に敷く面。位置と幅は Base UI が CSS 変数で渡す */}
        <Tabs.Indicator className="absolute top-1/2 left-0 -z-1 h-9 w-(--active-tab-width) translate-x-(--active-tab-left) -translate-y-1/2 rounded-full bg-card transition-[translate,width] duration-(--duration-slow) ease-out" />
      </Tabs.List>
      {(Object.keys(CHANNELS) as Channel[]).map((c) => (
        <Tabs.Panel
          key={c}
          value={c}
          ref={c === channel ? enter : undefined}
          className="flex flex-col gap-4 outline-none [&[hidden]]:hidden"
        >
          <p className={cx(textClass("muted"), "max-w-xl")}>{t(`${c}_body`)}</p>
          {body(c)}
        </Tabs.Panel>
      ))}
    </Tabs.Root>
  );
}
