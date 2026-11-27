import { Menu } from "@base-ui/react/menu";
import { Popover } from "@base-ui/react/popover";
import { Tooltip as BaseTooltip } from "@base-ui/react/tooltip";
import { type ReactElement, type ReactNode, useState } from "react";

import {
  DROPDOWN_PRESENCE,
  type Side,
  TOOLTIP_PRESENCE,
  usePopupPresence,
} from "@/ui/usePopupPresence";
import { cx, panelClass } from "@/ui/variants";

type PopupProps = {
  /** Root に渡している open。出入りのアニメーションに使う */
  open: boolean;
  side?: Side;
  align?: "start" | "center" | "end";
  className?: string;
  children: ReactNode;
};

const positioner = (side: Side, align: PopupProps["align"]) => ({
  className: "z-50",
  side,
  align,
  sideOffset: 8,
  // 画面を囲む帯の内側に収める
  collisionPadding: 12,
});

/** Menu.Root の中に置く。Portal から Popup までと、出入りのアニメーションをまとめたもの */
export function MenuPopup({
  open,
  side = "bottom",
  align = "end",
  className,
  children,
}: PopupProps) {
  const ref = usePopupPresence<HTMLDivElement>(open, DROPDOWN_PRESENCE[side]);
  return (
    <Menu.Portal>
      <Menu.Positioner {...positioner(side, align)}>
        <Menu.Popup ref={ref} className={cx(panelClass, "rounded-xl p-1 outline-none", className)}>
          {children}
        </Menu.Popup>
      </Menu.Positioner>
    </Menu.Portal>
  );
}

/** Popover.Root の中に置く。MenuPopup の Popover 版 */
export function PopoverPopup({
  open,
  side = "bottom",
  align = "end",
  className,
  children,
}: PopupProps) {
  const ref = usePopupPresence<HTMLDivElement>(open, DROPDOWN_PRESENCE[side]);
  return (
    <Popover.Portal>
      <Popover.Positioner {...positioner(side, align)}>
        {/* マウスで開いたときに中の最初のリンクへフォーカスを移さない（フォーカスリングが出る） */}
        <Popover.Popup
          ref={ref}
          initialFocus={(type) => type === "keyboard"}
          className={cx(panelClass, "rounded-xl p-2 outline-none", className)}
        >
          {children}
        </Popover.Popup>
      </Popover.Positioner>
    </Popover.Portal>
  );
}

/** アイコンだけのボタンに、aria-label と同じ文言のツールチップを付ける */
export function Tooltip({ label, children }: { label: string; children: ReactElement }) {
  const [open, setOpen] = useState(false);
  const ref = usePopupPresence<HTMLDivElement>(open, TOOLTIP_PRESENCE.bottom);
  return (
    <BaseTooltip.Root open={open} onOpenChange={setOpen}>
      <BaseTooltip.Trigger render={children} />
      <BaseTooltip.Portal>
        <BaseTooltip.Positioner className="z-50" side="bottom" sideOffset={6}>
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
