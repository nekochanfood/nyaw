import { Dialog } from "@base-ui/react/dialog";
import { GithubLogo } from "@phosphor-icons/react/dist/csr/GithubLogo";
import { List } from "@phosphor-icons/react/dist/csr/List";
import { X } from "@phosphor-icons/react/dist/csr/X";
import { Link } from "@tanstack/react-router";
import { animate, stagger } from "animejs";
import { useCallback, useEffect, useLayoutEffect, useRef } from "react";

import { useT } from "@/i18n/index";
import { REPO_URL } from "@/links";
import { duration } from "@/motion/duration";
import { reducedMotion } from "@/motion/reducedMotion";
import { slideFrom, usePopupPresence } from "@/ui/usePopupPresence";
import { buttonClass } from "@/ui/variants";
import { useNavItems } from "@/layout/nav";

/** メニューの開閉で、項目が動く距離（px）。ヘッダーの項目（Header）とメニューの項目で同じにする */
export const MENU_ITEM_SHIFT = 24;

/**
 * 面の出入り。面は帯と同じ色で動きが見えないので、ずらさずに薄れるだけにする（ずらすと、中の項目の動く量が
 * ヘッダーの項目とずれる）。閉じるときは、項目が消え始めてから面を薄れさせ、下の本文のカードが
 * ゆっくり見えてくるようにする（既定の in(3) では、最後の一瞬で消えたように見える）
 */
const PRESENCE = { ...slideFrom("bottom", 0, "slow", "slow"), exitEase: "out(2)", exitDelay: 120 };

/**
 * 狭い画面のメニュー。ヘッダーの下から画面の下端までを帯と同じ色で覆い、本文のカードを隠して、帯の上にページの一覧を
 * 大きく並べる。開閉の状態はヘッダーが持ち、開いている間はロゴと右のボタンを引っ込める（Header）
 */
export function MobileMenu({ open, setOpen }: { open: boolean; setOpen: (open: boolean) => void }) {
  const t = useT("nav");
  const items = useNavItems();
  const iconRef = useRef<HTMLSpanElement>(null);
  const iconWas = useRef(open);
  // ハンバーガーと X を切り替えるたびに、新しいアイコンが少し行き過ぎながら回って入る
  useEffect(() => {
    if (iconWas.current === open) return;
    iconWas.current = open;
    if (!iconRef.current || reducedMotion()) return;
    const anim = animate(iconRef.current, {
      rotate: [-90, 0],
      duration: duration("normal"),
      ease: "outBack(1.7)",
    });
    return () => {
      anim.cancel();
    };
  }, [open]);
  const presence = usePopupPresence<HTMLDivElement>(open, PRESENCE);
  const popupRef = useRef<HTMLDivElement | null>(null);
  const justMounted = useRef(false);
  // 開いたときに、項目が上から順に入る
  const ref = useCallback(
    (el: HTMLDivElement | null) => {
      presence(el);
      popupRef.current = el;
      if (!el) return;
      justMounted.current = true;
      if (reducedMotion()) return;
      animate(el.querySelectorAll("[data-menu-item]"), {
        opacity: [0, 1],
        y: [MENU_ITEM_SHIFT, 0],
        duration: 500,
        delay: stagger(60, { start: 80 }),
        ease: "out(4)",
      });
    },
    [presence],
  );
  /*
    閉じるときは、項目が下から順に沈みながら消え、少し遅れて面が薄れる（PRESENCE）。
    閉じている途中で開き直したときは戻す
  */
  useLayoutEffect(() => {
    const el = popupRef.current;
    const mounted = justMounted.current;
    justMounted.current = false;
    if (!el || (open && mounted) || reducedMotion()) return;
    const anim = animate(el.querySelectorAll("[data-menu-item]"), {
      opacity: open ? 1 : 0,
      y: open ? 0 : MENU_ITEM_SHIFT,
      duration: duration("normal"),
      delay: stagger(40, { reversed: !open }),
      ease: open ? "out(3)" : "in(2)",
    });
    return () => {
      anim.cancel();
    };
  }, [open]);
  const close = () => setOpen(false);
  return (
    <Dialog.Root open={open} onOpenChange={setOpen}>
      <Dialog.Trigger
        aria-label={open ? t("close_menu") : t("open_menu")}
        className={buttonClass({ variant: "onFrame" })}
      >
        <span ref={iconRef} className="inline-flex">
          {open ? <X size={20} weight="bold" /> : <List size={20} weight="bold" />}
        </span>
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Popup
          ref={ref}
          // タップやクリックで開いたときに、最初のリンクにフォーカスの枠を出さない。キーボードのときだけ中へ移す
          initialFocus={(type) => type === "keyboard"}
          className="frame-x fixed inset-x-0 top-(--frame-top) bottom-0 z-(--layer-menu) flex flex-col gap-2 overflow-y-auto bg-frame px-5 pt-4 pb-8 outline-none"
        >
          <Dialog.Title className="sr-only">{t("open_menu")}</Dialog.Title>
          <nav className="flex flex-col">
            {items.map((item) => (
              <Link
                key={item.to}
                data-menu-item
                to={item.to}
                params={item.params}
                hash={item.hash}
                onClick={close}
                className="border-b border-neutral-border-subtle py-4 text-3xl font-bold"
              >
                {item.label}
              </Link>
            ))}
            <a
              data-menu-item
              href={REPO_URL}
              target="_blank"
              rel="noopener"
              className="flex items-center gap-3 py-4 text-3xl font-bold"
            >
              <GithubLogo size={28} />
              {t("github")}
            </a>
          </nav>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
