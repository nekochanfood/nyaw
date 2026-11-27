/*
  見た目の variant をまとめる場所。同じクラスの組み合わせが 2 か所以上に出てきたら、ここに名前を付けて足す。
  呼び出し側の className は最後に足すだけで、ここの値を上書きしない。配置や幅など、variant が持たない性質だけを渡す。
*/

export const cx = (...classes: (string | false | null | undefined)[]) =>
  classes.filter(Boolean).join(" ");

const FADE = "transition-colors duration-(--duration-slow)";

export const BUTTON_VARIANTS = {
  /** 地の上に直接置くアイコン・文字ボタン。ホバーの間だけ面が出る */
  ghost: `hover:bg-control-on-background data-popup-open:bg-control-on-background ${FADE}`,
  /** 面のあるボタン */
  surface: `bg-control hover:bg-control-hover active:bg-control-active ${FADE}`,
  /** 本文色で塗った強調ボタン。文字は面の色で抜く */
  solid: `bg-neutral-text text-card hover:bg-neutral-text/85 ${FADE}`,
  /** 文字だけのリンク。ホバーで本文色になる */
  text: `text-neutral-text-low hover:text-neutral-text ${FADE}`,
} as const;

export const BUTTON_SIZES = {
  /** 大きさは呼び出し側が持つ */
  none: "",
  /** 20px のアイコン */
  icon: "size-10 inline-flex items-center justify-center",
  md: "h-10 px-4 text-sm font-medium inline-flex items-center gap-2",
  lg: "h-12 px-6 text-base font-medium inline-flex items-center gap-2",
} as const;

export type ButtonStyle = {
  variant?: keyof typeof BUTTON_VARIANTS;
  size?: keyof typeof BUTTON_SIZES;
  className?: string;
};

export function buttonClass({ variant = "ghost", size = "icon", className }: ButtonStyle = {}) {
  return cx(
    "rounded-full select-none disabled:opacity-50 cursor-pointer",
    BUTTON_VARIANTS[variant],
    BUTTON_SIZES[size],
    className,
  );
}

export const TEXT_VARIANTS = {
  /** ページの最上部の大見出し */
  display: "font-serif text-4xl sm:text-5xl lg:text-6xl leading-[1.1] tracking-tight text-balance",
  /** 区画の見出し */
  section: "font-serif text-3xl sm:text-4xl leading-tight tracking-tight text-balance",
  /** カードの見出し */
  card: "text-lg font-bold",
  /** 見出しの上に添える小さな文字 */
  eyebrow: "text-sm font-medium text-neutral-text-low",
  /** 本文より一段大きい導入文 */
  lead: "text-lg leading-relaxed text-neutral-text-low text-pretty",
  /** 控えめな説明文 */
  muted: "text-sm leading-relaxed text-neutral-text-low text-pretty",
} as const;

export function textClass(variant: keyof typeof TEXT_VARIANTS, className?: string) {
  return cx(TEXT_VARIANTS[variant], className);
}

/** 浮いたパネル。ドロップダウンやポップオーバー */
export const panelClass = "bg-card text-neutral-text border border-neutral-border-subtle";

/** メニューの項目。ホバーでもキーボードでも data-highlighted で同じ面が出る */
export const menuItemClass =
  "flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm outline-none cursor-pointer data-highlighted:bg-control-on-background";

/** 本文の幅。ヘッダー・本文・フッターで揃える */
export const containerClass = "mx-auto w-full max-w-6xl px-5 sm:px-8";
