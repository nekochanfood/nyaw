import { useSyncExternalStore } from "react";

export type Theme = "light" | "dark";

const STORAGE_KEY = "nyaw-theme";
const DARK_QUERY = "(prefers-color-scheme: dark)";

/**
 * 描画の前に、保存したテーマを `<html data-theme>` に反映する（`<head>` に埋め込む）。
 * 保存していなければ属性を付けず、OS の設定に従う。
 */
export const themeScript = `try{var t=localStorage.getItem("${STORAGE_KEY}");if(t==="light"||t==="dark")document.documentElement.dataset.theme=t}catch(e){}`;

function current(): Theme {
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
  return useSyncExternalStore(subscribe, current, () => null);
}

/** テーマを切り替えてから、色のトランジションを外すまでの時間（styles.css の 300ms より少し長く） */
const FADE_MS = 350;
let fading = 0;

export function setTheme(theme: Theme) {
  const root = document.documentElement;
  // 切り替えの間だけ、すべての要素の色を同じ速さで変える（styles.css の data-theme-fading）
  root.dataset.themeFading = "";
  root.dataset.theme = theme;
  clearTimeout(fading);
  fading = window.setTimeout(() => delete root.dataset.themeFading, FADE_MS);
  try {
    localStorage.setItem(STORAGE_KEY, theme);
  } catch {
    // 保存できない環境（プライベートブラウズなど）では、このページの間だけ切り替える
  }
}
