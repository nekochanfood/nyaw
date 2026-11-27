import { Dialog } from "@base-ui/react/dialog";
import { GithubLogo } from "@phosphor-icons/react/dist/csr/GithubLogo";
import { List } from "@phosphor-icons/react/dist/csr/List";
import { X } from "@phosphor-icons/react/dist/csr/X";
import { Link } from "@tanstack/react-router";
import { animate, stagger } from "animejs";
import { useCallback, useEffect, useRef } from "react";

import { useT } from "@/i18n/index";
import { REPO_URL } from "@/links";
import { reducedMotion } from "@/motion/reducedMotion";
import { slideFrom, usePopupPresence } from "@/ui/usePopupPresence";
import { buttonClass } from "@/ui/variants";
import { useNavItems } from "@/layout/Header";

const PRESENCE = slideFrom("bottom", 16, 250, 180);

/**
 * 狭い画面のメニュー。ヘッダーの下を覆い、ページの一覧を大きく並べる。開閉の状態はヘッダーが持ち、
 * 開いている間はロゴと右のボタンを引っ込める（Header）
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
      duration: 250,
      ease: "outBack(1.7)",
    });
    return () => {
      anim.cancel();
    };
  }, [open]);
  const presence = usePopupPresence<HTMLDivElement>(open, PRESENCE);
  // 開いたときに、項目が上から順に入る
  const ref = useCallback(
    (el: HTMLDivElement | null) => {
      presence(el);
      if (!el || reducedMotion()) return;
      animate(el.querySelectorAll("[data-menu-item]"), {
        opacity: [0, 1],
        y: [16, 0],
        duration: 500,
        delay: stagger(60, { start: 80 }),
        ease: "out(4)",
      });
    },
    [presence],
  );
  const close = () => setOpen(false);
  return (
    <Dialog.Root open={open} onOpenChange={setOpen}>
      <Dialog.Trigger
        aria-label={open ? t("close_menu") : t("open_menu")}
        className={buttonClass()}
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
          className="fixed inset-x-(--frame-gutter) top-[calc(var(--frame-gutter)+4rem)] bottom-(--frame-gutter) z-30 flex flex-col gap-2 overflow-y-auto rounded-b-(--frame-radius) bg-card px-5 pt-4 pb-8 outline-none"
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
