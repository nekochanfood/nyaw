import { describe, expect, it } from "vitest";

import { fadeDelay, rippleRadius } from "@/ui/ripple";

describe("rippleRadius", () => {
  it("中央から押すと対角線の半分になる", () => {
    expect(rippleRadius(100, 50, 50, 25)).toBeCloseTo(Math.hypot(50, 25));
  });

  it("角から押すと反対の角まで届く", () => {
    expect(rippleRadius(100, 50, 0, 0)).toBeCloseTo(Math.hypot(100, 50));
    expect(rippleRadius(100, 50, 100, 50)).toBeCloseTo(Math.hypot(100, 50));
  });

  it("どこから押しても 4 隅すべてを覆う", () => {
    const [w, h, x, y] = [120, 40, 30, 10];
    const r = rippleRadius(w, h, x, y);
    for (const [cx, cy] of [
      [0, 0],
      [w, 0],
      [0, h],
      [w, h],
    ]) {
      expect(Math.hypot(cx - x, cy - y)).toBeLessThanOrEqual(r);
    }
  });
});

describe("fadeDelay", () => {
  it("すぐに離しても、最低限広がってから消える", () => {
    expect(fadeDelay(0)).toBe(150);
    expect(fadeDelay(100)).toBe(50);
  });

  it("十分に押していたら、待たずに消える", () => {
    expect(fadeDelay(150)).toBe(0);
    expect(fadeDelay(1000)).toBe(0);
  });
});
