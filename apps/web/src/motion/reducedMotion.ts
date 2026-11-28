/** 視差効果を減らす設定。動きは省き、終わりの状態だけを見せる */
export const reducedMotion = () =>
  typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches;
