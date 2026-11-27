import { Dialog } from "@base-ui/react/dialog";
import { GithubLogo } from "@phosphor-icons/react/dist/csr/GithubLogo";
import { List } from "@phosphor-icons/react/dist/csr/List";
import { X } from "@phosphor-icons/react/dist/csr/X";
import { Link } from "@tanstack/react-router";
import { animate, stagger } from "animejs";
import { useCallback, useState } from "react";

import { useT } from "@/i18n/index";
import { REPO_URL } from "@/links";
import { reducedMotion } from "@/motion/reducedMotion";
import { slideFrom, usePopupPresence } from "@/ui/usePopupPresence";
import { buttonClass } from "@/ui/variants";
import { useNavItems } from "@/layout/Header";

const PRESENCE = slideFrom("bottom", 16, 250, 180);

/** 狭い画面のメニュー。ヘッダーの下を覆い、ページの一覧を大きく並べる */
export function MobileMenu() {
  const t = useT("nav");
  const items = useNavItems();
  const [open, setOpen] = useState(false);
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
        {open ? <X size={20} weight="bold" /> : <List size={20} weight="bold" />}
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Popup
          ref={ref}
          className="fixed inset-x-(--frame-gutter) top-[calc(var(--frame-gutter)+4rem)] bottom-(--frame-gutter) z-30 flex flex-col gap-2 overflow-y-auto rounded-b-(--frame-radius) bg-card px-5 pt-4 pb-8 outline-none"
        >
          <Dialog.Title className="sr-only">{t("open_menu")}</Dialog.Title>
          <nav className="flex flex-col">
            {items.map((item) => (
              <Link
                key={item.label}
                data-menu-item
                to={item.to}
                params={item.params}
                hash={item.hash}
                onClick={close}
                className="border-b border-neutral-border-subtle py-4 font-serif text-3xl"
              >
                {item.label}
              </Link>
            ))}
            <a
              data-menu-item
              href={REPO_URL}
              target="_blank"
              rel="noopener"
              className="flex items-center gap-3 py-4 font-serif text-3xl"
            >
              <GithubLogo size={28} />
              {t("github")}
            </a>
          </nav>
          <Dialog.Close
            className={buttonClass({
              variant: "surface",
              size: "md",
              className: "mt-auto self-start",
            })}
          >
            <X size={16} weight="bold" />
            {t("close_menu")}
          </Dialog.Close>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
