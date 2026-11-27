import { describe, expect, it } from "vitest";

import { quickSettingsOpacity, SHOTS, shotAt, stepAt } from "@/scene/path";

describe("shotAt", () => {
  it("両端と各段の位置で、その段のカメラになる", () => {
    SHOTS.forEach((shot, i) => {
      expect(shotAt(i / (SHOTS.length - 1))).toEqual(shot);
    });
  });

  it("範囲の外の進みは端に留める", () => {
    expect(shotAt(-1)).toEqual(SHOTS[0]);
    expect(shotAt(2)).toEqual(SHOTS.at(-1));
  });

  it("段の間では、隣り合う位置の間にある", () => {
    const { position } = shotAt(0.125);
    for (const axis of [0, 1, 2]) {
      const [a, b] = [SHOTS[0].position[axis], SHOTS[1].position[axis]];
      expect(position[axis]).toBeGreaterThanOrEqual(Math.min(a, b));
      expect(position[axis]).toBeLessThanOrEqual(Math.max(a, b));
    }
  });
});

describe("stepAt", () => {
  it("カメラが寄った段を返す", () => {
    expect(stepAt(0)).toBe(0);
    expect(stepAt(0.25)).toBe(0);
    expect(stepAt(0.5)).toBe(1);
    expect(stepAt(0.75)).toBe(2);
    expect(stepAt(1)).toBe(3);
  });
});

describe("quickSettingsOpacity", () => {
  it("ミラーリングの段では隠れ、クイック設定の段より先では見える", () => {
    expect(quickSettingsOpacity(0)).toBe(0);
    expect(quickSettingsOpacity(0.5)).toBe(0);
    expect(quickSettingsOpacity(0.75)).toBe(1);
    expect(quickSettingsOpacity(1)).toBe(1);
  });

  it("途中では単調に増える", () => {
    let last = 0;
    for (let p = 0.5; p <= 0.75; p += 0.01) {
      const o = quickSettingsOpacity(p);
      expect(o).toBeGreaterThanOrEqual(last);
      last = o;
    }
  });
});
