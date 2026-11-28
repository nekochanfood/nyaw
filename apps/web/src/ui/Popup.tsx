import { Menu } from "@base-ui/react/menu";
import { Tooltip as BaseTooltip } from "@base-ui/react/tooltip";
import { type ReactElement, type ReactNode, useState } from "react";

import { DROPDOWN_PRESENCE, TOOLTIP_PRESENCE, usePopupPresence } from "@/ui/usePopupPresence";
import { cx, panelClass, POPUP_INSET } from "@/ui/variants";

type PopupProps = {
  /** Root に渡している open。出入りのアニメーションに使う */
  open: boolean;
  className?: string;
  children: ReactNode;
};

/**
 * Menu.Root の中に置く。Portal から Popup までと、出入りのアニメーションをまとめたもの。
 * トリガーの下に、右端をそろえて開く
 */
export function MenuPopup({ open, className, children }: PopupProps) {
  const ref = usePopupPresence<HTMLDivElement>(open, DROPDOWN_PRESENCE);
  return (
    <Menu.Portal>
      <Menu.Positioner
        className="z-(--layer-popup)"
        side="bottom"
        align="end"
        sideOffset={8}
        // 画面を囲む帯の内側に収める
        collisionPadding={12}
      >
        <Menu.Popup
          ref={ref}
          className={cx(panelClass, POPUP_INSET.menu, "outline-none", className)}
        >
          {children}
        </Menu.Popup>
      </Menu.Positioner>
    </Menu.Portal>
  );
}

/** アイコンだけのボタンに、aria-label と同じ文言のツールチップを付ける */
export function Tooltip({ label, children }: { label: string; children: ReactElement }) {
  const [open, setOpen] = useState(false);
  const ref = usePopupPresence<HTMLDivElement>(open, TOOLTIP_PRESENCE);
  return (
    <BaseTooltip.Root open={open} onOpenChange={setOpen}>
      <BaseTooltip.Trigger render={children} />
      <BaseTooltip.Portal>
        <BaseTooltip.Positioner className="z-(--layer-popup)" side="bottom" sideOffset={6}>
          <BaseTooltip.Popup
            ref={ref}
            className="rounded-lg bg-tooltip px-2.5 py-1 text-xs text-on-tooltip"
          >
            {label}
          </BaseTooltip.Popup>
        </BaseTooltip.Positioner>
      </BaseTooltip.Portal>
    </BaseTooltip.Root>
  );
}

export const TooltipProvider = BaseTooltip.Provider;
