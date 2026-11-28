import { useEffect, useRef, useState } from "react";

import { Logo } from "@/layout/Logo";
import { reducedMotion } from "@/motion/reducedMotion";
import { currentTheme, type Theme, useTheme } from "@/theme";
import { cx } from "@/ui/variants";

/*
  フルロゴのイントロ（assets/animations/logo-full）。最初に開いたときは読み込み画面の中央で再生し、ランディングページでは
  再生した DOM をそのままページのロゴの置き場所（IntroLogoSlot）へ移して使い続ける。この流れは Loading.tsx が進める。
  別のページからランディングページへ移ってきたときは、置き場所が自分でイントロを再生する。
*/

/** フルロゴの viewBox の大きさ（assets/logos の SVG） */
const VIEW_BOX = { w: 352.49, h: 150.19 };
/** Lottie の合成の四辺にある余白（assets/animations/logo-full/build.mjs の MARGIN） */
const MARGIN = 24;

/**
 * イントロのロゴの大きさ。読み込み画面とランディングページの置き場所で同じにし、移るときに大きさが変わらないようにする。
 * ページの主役として見える大きさにしつつ、狭い画面では幅の 72% に収める
 */
export const introLogoClass = "relative aspect-[352.49/150.19] w-[min(34rem,72vw)]";

/** 再生の準備ができたイントロのロゴ */
export interface IntroLogo {
  /** ロゴの外形の大きさの箱（introLogoClass）。合成の余白の分は、中で外側へはみ出す */
  node: HTMLDivElement;
  theme: Theme;
  /**
   * イントロの区間を再生する。再生し終えたら、耳やしっぽを押すと、その部品がぴくっとするようになる
   * （押せる範囲と反応の区間は、Lottie の中にある。assets/animations/logo-full/README.md を参照）
   */
  play: () => Promise<void>;
  destroy: () => void;
}

/** テーマに合ったイントロを読み込み、最初のフレームで止めた状態で返す */
export async function loadIntroLogo(theme: Theme): Promise<IntroLogo> {
  const [{ default: lottie }, { default: animationData }] = await Promise.all([
    import("lottie-web/build/player/lottie_svg"),
    theme === "dark"
      ? import("@assets/animations/logo-full/logo-full-dark.json")
      : import("@assets/animations/logo-full/logo-full-light.json"),
  ]);
  const node = document.createElement("div");
  node.className = introLogoClass;
  node.setAttribute("role", "img");
  node.setAttribute("aria-label", "nyaw");
  // 合成は、ロゴの外形の四辺に余白を足した大きさ。余白の分だけ外側へ広げ、ロゴの外形を箱に合わせる
  const stage = document.createElement("div");
  Object.assign(stage.style, {
    position: "absolute",
    left: `${(-MARGIN / VIEW_BOX.w) * 100}%`,
    top: `${(-MARGIN / VIEW_BOX.h) * 100}%`,
    width: `${(animationData.w / VIEW_BOX.w) * 100}%`,
    height: `${(animationData.h / VIEW_BOX.h) * 100}%`,
  });
  node.append(stage);
  const anim = lottie.loadAnimation({
    container: stage,
    renderer: "svg",
    loop: false,
    autoplay: false,
    animationData,
  });
  // 押せる範囲（class が "intro-hit <部品>" の path）を押したら、その部品の反応の区間（"twitch-<部品>"）を再生する。
  // イントロの再生中は受け付けない
  node.addEventListener("click", (e) => {
    const hit = e.target instanceof Element && e.target.closest(".intro-hit");
    if (!hit || !("introReady" in node.dataset)) return;
    anim.playSegments(segment(animationData, `twitch-${hit.classList[1]}`), true);
  });
  // lottie-web は SVG に translate3d を付けて、独立した描画の層にする。外側の要素の位置が変わると、この層が端数のピクセル位置で
  // 描き直され、箱の端に古い画素が残る。層を分けず、外側の要素と一緒に描かせる
  stage.querySelector("svg")?.style.setProperty("transform", "none");
  return {
    node,
    theme,
    play: () =>
      new Promise((resolve) => {
        const done = () => {
          anim.removeEventListener("complete", done);
          node.dataset.introReady = "";
          resolve();
        };
        anim.addEventListener("complete", done);
        anim.playSegments(segment(animationData, "intro"), true);
      }),
    destroy: () => anim.destroy(),
  };
}

/** Lottie のマーカーの区間を [始め, 終わり] で返す */
function segment(data: { markers: { tm: number; cm: string; dr: number }[] }, name: string): [number, number] {
  const marker = data.markers.find((m) => m.cm === name);
  if (!marker) throw new Error(`イントロに区間 ${name} がない`);
  return [marker.tm, marker.tm + marker.dr];
}

/** ランディングページの置き場所。イントロを受け取れるのは、今表示している 1 つだけ */
let slot: ((logo: IntroLogo) => void) | null = null;
let slotHost: HTMLElement | null = null;

/** 今のページにイントロの置き場所があれば、その要素（ロゴの外形の箱）を返す */
export const introSlotElement = () => slotHost;

/** 置き場所へイントロのロゴを渡す。置き場所がなければ false を返す */
export function adoptIntroLogo(logo: IntroLogo) {
  if (!slot) return false;
  slot(logo);
  return true;
}

/**
 * ランディングページのロゴの置き場所。イントロを受け取ったらそれを、受け取っていなければ静止画のロゴを出す。
 * - `pending` の間（最初に開いたときで、読み込み画面のイントロがこれから渡される）は、静止画も出さずに場所だけを取っておく。
 * - 別のページから移ってきたとき（置いた時点で `pending` でない）は、自分でイントロを読み込み、ページが見え始めたら再生する。
 *   読み込む間は場所だけを取っておき、動きを減らす設定のときと、読み込めないときは静止画を出す。
 * - 受け取ったあとでテーマを切り替えたら、色が合わないのでイントロを隠して静止画を出す（テーマを戻せば、またイントロを出す）
 */
export function IntroLogoSlot({ pending }: { pending: boolean }) {
  const box = useRef<HTMLDivElement>(null);
  const host = useRef<HTMLDivElement>(null);
  const [adopted, setAdopted] = useState<IntroLogo | null>(null);
  // サーバーと、最初に開いたときのハイドレーションでは pending が true なので、どちらでも false になり食い違わない
  const [selfPlay] = useState(() => !pending && !reducedMotion());
  const [failed, setFailed] = useState(false);
  const theme = useTheme();

  useEffect(() => {
    const adopt = (logo: IntroLogo) => {
      // 箱の大きさは置き場所が決めるので、ロゴの中身だけを置き場所の箱いっぱいに広げる
      logo.node.removeAttribute("style");
      logo.node.className = "absolute inset-0";
      host.current?.append(logo.node);
      setAdopted(logo);
    };
    let own: IntroLogo | undefined;
    let cancelled = false;
    if (selfPlay) {
      loadIntroLogo(currentTheme()).then(
        async (logo) => {
          if (cancelled) return logo.destroy();
          own = logo;
          adopt(logo);
          await pageVisible(box.current);
          if (!cancelled) await logo.play();
        },
        () => setFailed(true),
      );
    } else {
      slotHost = box.current;
      slot = (logo) => {
        own = logo;
        adopt(logo);
      };
    }
    return () => {
      cancelled = true;
      slot = null;
      slotHost = null;
      own?.destroy();
    };
  }, [selfPlay]);

  const showIntro = adopted !== null && (theme === null || theme === adopted.theme);
  const waiting = pending || (selfPlay && !failed && !adopted);
  return (
    <div ref={box} className={introLogoClass}>
      <div data-intro-static className={cx("absolute inset-0", (waiting || showIntro) && "invisible")}>
        <Logo className="h-full w-full" />
      </div>
      {/*
        イントロの入れ物。隠すのは、受け取ったあとでテーマが合わなくなったときだけにする。受け取る前から隠していると、
        受け取ってから React が描き直すまでの 1 フレームほど、ロゴが消えてちらつく
      */}
      <div ref={host} className={cx("absolute inset-0", adopted && !showIntro && "invisible")} />
    </div>
  );
}

/** el を含むページ（PageTransition の [data-page]）のフェードインが始まるまで待つ */
function pageVisible(el: HTMLElement | null) {
  const page = el?.closest<HTMLElement>("[data-page]");
  return new Promise<void>((resolve) => {
    const tick = () => {
      if (!page?.isConnected || Number(getComputedStyle(page).opacity) > 0) resolve();
      else requestAnimationFrame(tick);
    };
    tick();
  });
}
