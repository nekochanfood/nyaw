import { animate } from "animejs";
import { createContext, type ReactNode, useContext, useEffect, useState } from "react";

import {
  adoptIntroLogo,
  type IntroLogo,
  introSlotElement,
  loadIntroLogo,
} from "@/layout/IntroLogo";
import { duration } from "@/motion/duration";
import { reducedMotion } from "@/motion/reducedMotion";
import { currentTheme } from "@/theme";

const LoadingContext = createContext({ done: false, intro: true });

/** 読み込み画面が消え始めたか。ページの最初の動きは、これが true になってから始める */
export const useLoadingDone = () => useContext(LoadingContext).done;

/** イントロのロゴを再生中で、これからランディングページの置き場所へ渡すか（その間、置き場所は空けておく） */
export const useIntroPending = () => useContext(LoadingContext).intro;

/** ランディングページで、イントロのロゴが置き場所へ移る時間（ms） */
const SLIDE_MS = 800;
/** 移り終えてから、ページのフェードインが終わるのを待つ上限（ms） */
const SETTLE_LIMIT_MS = 2000;

const easeInOutCubic = (u: number) => (u < 0.5 ? 4 * u ** 3 : 1 - (2 - 2 * u) ** 3 / 2);

/** CSS の色（rgb() か rgba()）を、同じ色の透明にする */
const transparent = (color: string) =>
  color.replace(/rgba?\(([^,]+),([^,]+),([^,)]+).*\)/, "rgba($1,$2,$3,0)");

/**
 * 読み込み画面を進める。ランディングページ（イントロの置き場所があるページ）では、HTML を読み終えた時点
 * （DOMContentLoaded）で、スピナーをフルロゴのイントロに替えて再生する。背景は再生が終わるまで残す。
 * 画像やフォントの読み込みは待たない。
 *
 * 再生が終わったら、背景をフェードアウトしながら、再生したロゴをそのまま置き場所へ滑らかに移す。
 * ほかのページのとき、動きを減らす設定のとき、イントロを読み込めないときは、再生せずに読み込み画面をフェードして消す
 */
export function LoadingProvider({ children }: { children: ReactNode }) {
  const [done, setDone] = useState(false);
  const [intro, setIntro] = useState(true);
  useEffect(() => {
    const screen = document.getElementById("loading-screen");
    let cancelled = false;
    const anims: ReturnType<typeof animate>[] = [];
    const fade = (el: HTMLElement, props: Parameters<typeof animate>[1]) => {
      const anim = animate(el, {
        duration: reducedMotion() ? 0 : duration("normal"),
        ease: "out(2)",
        ...props,
      });
      anims.push(anim);
      return anim;
    };

    const run = async () => {
      if (!screen) {
        setIntro(false);
        setDone(true);
        return;
      }
      const release = () => {
        screen.setAttribute("aria-busy", "false");
        screen.style.pointerEvents = "none";
      };
      const hide = () => {
        screen.style.display = "none";
      };
      // ページの最初の動きは、読み込み画面のフェードと同時に始める（終わってからだと、一瞬だけ本文が見える）
      const plain = async () => {
        setIntro(false);
        release();
        setDone(true);
        await fade(screen, { opacity: 0 });
        hide();
      };

      // イントロはランディングページだけで出す。置き場所の登録（introSlotElement）はハイドレーション後なので、
      // この時点では HTML にある置き場所の静止画で見分ける
      if (reducedMotion() || !document.querySelector("[data-intro-static]")) return plain();
      let logo: IntroLogo;
      try {
        logo = await loadIntroLogo(currentTheme());
      } catch {
        return plain();
      }
      if (cancelled) return;
      screen.querySelector<SVGElement>(":scope > svg")?.style.setProperty("display", "none");
      screen.append(logo.node);
      await logo.play();
      if (cancelled) return;

      // 再生中にほかのページへ移ると、置き場所がなくなる
      const target = introSlotElement();
      if (!target) {
        await plain();
        logo.destroy();
        return;
      }

      release();
      setDone(true);
      fade(screen, { backgroundColor: transparent(getComputedStyle(screen).backgroundColor) });
      await slide(logo.node, target, () => cancelled);
      if (!adoptIntroLogo(logo)) logo.destroy();
      hide();
      setIntro(false);
    };

    const start = () => void run();
    if (document.readyState === "loading") {
      document.addEventListener("DOMContentLoaded", start, { once: true });
    } else {
      start();
    }
    return () => {
      cancelled = true;
      document.removeEventListener("DOMContentLoaded", start);
      for (const anim of anims) anim.cancel();
    };
  }, []);
  return <LoadingContext.Provider value={{ done, intro }}>{children}</LoadingContext.Provider>;
}

/**
 * node を、今の位置から target の位置と大きさへ滑らかに移す。target はページのフェードインで動くので、毎フレーム測り直して追う。
 * 移り終えたあとも、ページのフェードインが終わるまで追い続ける（途中で渡すと、ページと一緒に半透明になる）。
 * 位置と大きさは最初に一度だけ決め、あとは transform で動かす。配置からやり直さずに済み、移動量をデバイスのピクセルに
 * そろえられる（端数のピクセル位置で描くと、箱の端に古い画素が残ることがある）
 */
function slide(node: HTMLElement, target: HTMLElement, cancelled: () => boolean) {
  const from = node.getBoundingClientRect();
  const page = target.closest<HTMLElement>("[data-page]");
  const snap = (v: number) => Math.round(v * devicePixelRatio) / devicePixelRatio;
  Object.assign(node.style, {
    position: "fixed",
    left: `${from.left}px`,
    top: `${from.top}px`,
    width: `${from.width}px`,
    height: `${from.height}px`,
    transformOrigin: "0 0",
  });
  return new Promise<void>((resolve) => {
    const begin = performance.now();
    const tick = (now: number) => {
      const k = easeInOutCubic(Math.min((now - begin) / SLIDE_MS, 1));
      const to = target.getBoundingClientRect();
      const dx = snap((to.left - from.left) * k);
      const dy = snap((to.top - from.top) * k);
      const sx = 1 + (to.width / from.width - 1) * k;
      const sy = 1 + (to.height / from.height - 1) * k;
      node.style.transform = `translate(${dx}px, ${dy}px) scale(${sx}, ${sy})`;
      const elapsed = now - begin;
      const settled = elapsed >= SLIDE_MS && (!page || getComputedStyle(page).opacity === "1");
      if (cancelled() || settled || elapsed >= SLIDE_MS + SETTLE_LIMIT_MS) resolve();
      else requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
}

/*
  CSS を読み込む前から描けるよう、読み込み画面の見た目はここに直接書く。色は styles.css の役割（--card など）が
  読み込まれていればそれを、まだなら Catppuccin の値（ライトは Latte、ダークは Frappé）を直接使う。
  ダークは保存したテーマか OS の設定で決まる。スピナーの指定は直下の SVG に限る（読み込み画面に置くイントロの SVG に
  当てると、回転してしまう）。
*/
const STYLE = `
#loading-screen{position:fixed;inset:0;z-index:100;display:flex;align-items:center;justify-content:center;background:var(--card,#eff1f5)}
#loading-screen>svg circle{stroke:var(--neutral-border,#9ca0b0)}
#loading-screen>svg circle+circle{stroke:var(--neutral-text,#4c4f69)}
#loading-screen>svg{animation:loading-spin .8s linear infinite}
@keyframes loading-spin{to{transform:rotate(360deg)}}
@media (prefers-color-scheme:dark){:root:not([data-theme=light]) #loading-screen{background:var(--card,#303446)}:root:not([data-theme=light]) #loading-screen>svg circle{stroke:var(--neutral-border,#737994)}:root:not([data-theme=light]) #loading-screen>svg circle+circle{stroke:var(--neutral-text,#c6d0f5)}}
:root[data-theme=dark] #loading-screen{background:var(--card,#303446)}
:root[data-theme=dark] #loading-screen>svg circle{stroke:var(--neutral-border,#737994)}
:root[data-theme=dark] #loading-screen>svg circle+circle{stroke:var(--neutral-text,#c6d0f5)}
@media (prefers-reduced-motion:reduce){#loading-screen>svg{animation-duration:2.4s}}
#loading-screen .sr-only{position:absolute;width:1px;height:1px;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap}
`;

/** アクセスした直後に、ページの上に重ねる読み込み画面。ランディングページでは、読み込みを終えると中央でフルロゴのイントロを再生する */
export function LoadingScreen() {
  return (
    <>
      <style>{STYLE}</style>
      {/* JavaScript が動かない環境では消す処理が動かないので、最初から出さない */}
      <noscript>
        <style>
          {"#loading-screen{display:none}[data-intro-static]{visibility:visible!important}"}
        </style>
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
