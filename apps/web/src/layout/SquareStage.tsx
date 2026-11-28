import type { ReactNode } from "react";

import { cx } from "@/ui/variants";

/**
 * 帯の内側の表示領域に収まる最大の正方形を、上下左右の中央に置く。中身は className で並べる。
 * 一辺は、表示領域の幅と高さの短いほう（100cqmin）
 */
export function SquareStage({ className, children }: { className?: string; children: ReactNode }) {
  return (
    <div className="grid h-(--frame-height) w-full place-items-center [container-type:size]">
      <div className={cx("size-[100cqmin]", className)}>{children}</div>
    </div>
  );
}
