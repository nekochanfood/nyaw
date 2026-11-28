import { useSyncExternalStore } from "react";

import { reducedMotion } from "@/motion/reducedMotion";

export type Theme = "light" | "dark";

const STORAGE_KEY = "nyaw-theme";
const DARK_QUERY = "(prefers-color-scheme: dark)";

/**
 * 描画の前に、保存したテーマを `<html data-theme>` に反映する（`<head>` に埋め込む）。
 * 保存していなければ属性を付けず、OS の設定に従う。
 */
export const themeScript = `try{var t=localStorage.getItem("${STORAGE_KEY}");if(t==="light"||t==="dark")document.documentElement.dataset.theme=t}catch(e){}`;

/** 表示中のテーマ。ブラウザーでだけ呼べる */
export function currentTheme(): Theme {
  const set = document.documentElement.dataset.theme;
  if (set === "light" || set === "dark") return set;
  return matchMedia(DARK_QUERY).matches ? "dark" : "light";
}

function subscribe(notify: () => void) {
  const media = matchMedia(DARK_QUERY);
  const observer = new MutationObserver(notify);
  observer.observe(document.documentElement, { attributeFilter: ["data-theme"] });
  media.addEventListener("change", notify);
  return () => {
    observer.disconnect();
    media.removeEventListener("change", notify);
  };
}

/** 表示中のテーマ。サーバーでは決まらないので `null` を返す */
export function useTheme(): Theme | null {
  return useSyncExternalStore(subscribe, currentTheme, () => null);
}

/**
 * テーマを切り替える。要素ごとの色のトランジションは使わず、切り替える瞬間だけ全要素のトランジションを止めて
 * 色を一度に変え、見た目の変化は画面全体のクロスフェード（View Transitions API）で見せる。
 *
 * 要素ごとにトランジションさせると、親から継承した色（文字やアイコン）が補間の途中で毎フレーム変わり、
 * 自分の color のトランジションを持つ子孫がそれを新しい行き先とみなして作り直し続けるので、チカチカする。
 */
export function setTheme(theme: Theme) {
  const root = document.documentElement;
  const apply = () => {
    root.dataset.themeSwitching = "";
    root.dataset.theme = theme;
    // トランジションを止めた状態で、新しいテーマのスタイルを確定させてから戻す（戻しても値は変わらない）
    void root.offsetWidth;
    delete root.dataset.themeSwitching;
  };
  if (document.startViewTransition && !reducedMotion()) {
    document.startViewTransition(apply);
  } else {
    apply();
  }
  try {
    localStorage.setItem(STORAGE_KEY, theme);
  } catch {
    // 保存できない環境（プライベートブラウズなど）では、このページの間だけ切り替える
  }
}
