import { animate } from "animejs";
import { createContext, type ReactNode, useContext, useEffect, useState } from "react";

import { duration } from "@/motion/duration";
import { reducedMotion } from "@/motion/reducedMotion";

const LoadingContext = createContext(false);

/** 読み込み画面が消え始めたか。ページの最初の動きは、これが true になってから始める */
export const useLoadingDone = () => useContext(LoadingContext);

/**
 * 読み込み画面を、HTML を読み終えた時点（DOMContentLoaded）でフェードして消す。
 * 画像やフォントの読み込みは待たない。
 */
export function LoadingProvider({ children }: { children: ReactNode }) {
  const [done, setDone] = useState(false);
  useEffect(() => {
    let anim: ReturnType<typeof animate> | undefined;
    const dismiss = () => {
      const el = document.getElementById("loading-screen");
      if (!el) return setDone(true);
      el.setAttribute("aria-busy", "false");
      el.style.pointerEvents = "none";
      // ページの最初の動きは、読み込み画面のフェードと同時に始める（終わってからだと、一瞬だけ本文が見える）
      setDone(true);
      anim = animate(el, {
        opacity: 0,
        duration: reducedMotion() ? 0 : duration("normal"),
        ease: "out(2)",
        onComplete: () => {
          el.style.display = "none";
        },
      });
    };
    if (document.readyState === "loading") {
      document.addEventListener("DOMContentLoaded", dismiss, { once: true });
    } else {
      dismiss();
    }
    return () => {
      document.removeEventListener("DOMContentLoaded", dismiss);
      anim?.cancel();
    };
  }, []);
  return <LoadingContext.Provider value={done}>{children}</LoadingContext.Provider>;
}

/*
  CSS を読み込む前から描けるよう、読み込み画面の見た目はここに直接書く。色はトークン（--card など）が
  読み込まれていればそれを、まだならその値を直接使う。ダークは保存したテーマか OS の設定で決まる。
*/
const STYLE = `
#loading-screen{position:fixed;inset:0;z-index:100;display:flex;align-items:center;justify-content:center;background:var(--card,#fff)}
#loading-screen circle{stroke:var(--neutral-border,#d9d9d9)}
#loading-screen circle+circle{stroke:var(--color-ctp-lavender,#7287fd)}
#loading-screen svg{animation:loading-spin .8s linear infinite}
@keyframes loading-spin{to{transform:rotate(360deg)}}
@media (prefers-color-scheme:dark){:root:not([data-theme=light]) #loading-screen{background:var(--card,oklch(.285 0 0))}}
:root[data-theme=dark] #loading-screen{background:var(--card,oklch(.285 0 0))}
@media (prefers-reduced-motion:reduce){#loading-screen svg{animation-duration:2.4s}}
#loading-screen .sr-only{position:absolute;width:1px;height:1px;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap}
`;

/** アクセスした直後に、ページの上に重ねる読み込み画面 */
export function LoadingScreen() {
  return (
    <>
      <style>{STYLE}</style>
      {/* JavaScript が動かない環境では消す処理が動かないので、最初から出さない */}
      <noscript>
        <style>{"#loading-screen{display:none}"}</style>
      </noscript>
      <output id="loading-screen" aria-busy="true">
        <svg aria-hidden width="48" height="48" viewBox="0 0 50 50">
          <circle cx="25" cy="25" r="20" fill="none" strokeWidth="6" />
          <circle
            cx="25"
            cy="25"
            r="20"
            fill="none"
            strokeWidth="6"
            strokeLinecap="round"
            strokeDasharray="31.4 94.2"
            transform="rotate(-90 25 25)"
          />
        </svg>
        <span className="sr-only">Loading / 読み込み中</span>
      </output>
    </>
  );
}
