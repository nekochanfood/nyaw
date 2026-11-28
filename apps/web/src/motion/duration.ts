/** トークンの時間の名前（tokens.json の duration-ms） */
export type Duration = "fast" | "quick" | "normal" | "slow";

/**
 * トークンの時間（ms）。tokens.css の `--duration-*` を読むので、CSS のトランジションと同じ値になる。
 * UI の状態の切り替え（ポップアップの出入り、パネルの切り替え）に使う。ページの演出（入場、スクロールに
 * 連動する動き）の時間は、それぞれの動きが持つ。
 */
export function duration(name: Duration) {
  const value = getComputedStyle(document.documentElement).getPropertyValue(`--duration-${name}`);
  return parseFloat(value) || 0;
}
