import fullDark from "@assets/logos/Logo_Full_Dark.svg?url";
import fullLight from "@assets/logos/Logo_Full_Light.svg?url";
import miniDark from "@assets/logos/Logo_Mini_Dark.svg?url";
import miniLight from "@assets/logos/Logo_Mini_Light.svg?url";
import { cx } from "@/ui/variants";

const SOURCES = { full: [fullLight, fullDark], mini: [miniLight, miniDark] } as const;

/**
 * テーマに合わせたロゴ。どちらも描画しておき、表示を CSS で切り替える（サーバーではテーマが分からない）。
 * `mini` は名前のない小さなロゴ。
 */
export function Logo({ kind = "full", className }: { kind?: "full" | "mini"; className?: string }) {
  const [light, dark] = SOURCES[kind];
  return (
    <>
      <img
        src={light}
        alt="nyaw"
        draggable={false}
        className={cx("select-none dark:hidden", className)}
      />
      <img
        src={dark}
        alt="nyaw"
        draggable={false}
        className={cx("hidden select-none dark:block", className)}
      />
    </>
  );
}
