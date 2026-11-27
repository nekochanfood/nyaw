/*
  スクロールの進み（0〜1）に応じたカメラの位置と各パネルの見え方。座標はアプリと同じく、目の位置を原点とする
  メートル単位の右手系（前が -Z、上が +Y）。
*/

export type Vec3 = [number, number, number];
export type Shot = { position: Vec3; target: Vec3 };

export const STEPS = ["dashboard", "mirroring", "quick_settings", "wrist"] as const;
export type Step = (typeof STEPS)[number];

/** 全体を斜め上から見る位置と、案内の各段で寄る位置。進み 0 から 1 までに等間隔で並ぶ */
export const SHOTS: Shot[] = [
  { position: [1.5, 0.55, 0.7], target: [-0.05, -0.3, -1.0] },
  { position: [0.35, -0.22, -0.5], target: [0, -0.52, -1.28] },
  { position: [0, 0.02, 0.15], target: [0, -0.12, -1.3] },
  { position: [-0.45, 0.08, -0.05], target: [0.05, -0.12, -1.3] },
  { position: [0.05, 0.02, 0.05], target: [-0.32, -0.2, -0.4] },
];

const clamp01 = (x: number) => Math.min(1, Math.max(0, x));
const smooth = (x: number) => x * x * (3 - 2 * x);
const mix = (a: Vec3, b: Vec3, t: number): Vec3 => [
  a[0] + (b[0] - a[0]) * t,
  a[1] + (b[1] - a[1]) * t,
  a[2] + (b[2] - a[2]) * t,
];

/** 進み `p` でのカメラ。隣り合う位置の間を、止まり際が緩やかになるように補間する */
export function shotAt(p: number): Shot {
  const x = clamp01(p) * (SHOTS.length - 1);
  // 段の位置ちょうどでは補間しない（誤差で位置がずれる）
  if (Number.isInteger(x)) return SHOTS[x];
  const i = Math.min(Math.floor(x), SHOTS.length - 2);
  const t = smooth(x - i);
  const [a, b] = [SHOTS[i], SHOTS[i + 1]];
  return { position: mix(a.position, b.position, t), target: mix(a.target, b.target, t) };
}

/** 進み `p` で説明している段（0〜3）。全体を見ている間は 0 */
export function stepAt(p: number): number {
  return Math.min(STEPS.length - 1, Math.max(0, Math.round(clamp01(p) * STEPS.length) - 1));
}

/**
 * 同じ場所に出るミラーリングの画面とクイック設定は、クイック設定の段へ向かう間に入れ替える。
 * 戻り値はクイック設定の不透明度で、ミラーリングの画面は 1 からこれを引いた値になる。
 */
export function quickSettingsOpacity(p: number) {
  const [from, to] = [2 / (SHOTS.length - 1), 3 / (SHOTS.length - 1)];
  return smooth(clamp01((p - from) / (to - from) / 0.5 - 0.5));
}
