import logoDark from "@assets/logos/Logo_Full_Dark.svg?url";
import logoLight from "@assets/logos/Logo_Full_Light.svg?url";
import { cx } from "@/ui/variants";

/** テーマに合わせたロゴ。どちらも描画しておき、表示を CSS で切り替える（サーバーではテーマが分からない） */
export function Logo({ className }: { className?: string }) {
  return (
    <>
      <img src={logoLight} alt="nyaw" className={cx("dark:hidden", className)} />
      <img src={logoDark} alt="nyaw" className={cx("hidden dark:block", className)} />
    </>
  );
}
