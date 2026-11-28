import { createLink } from "@tanstack/react-router";
import { type ComponentProps, forwardRef } from "react";

import { type ButtonStyle, buttonClass } from "@/ui/variants";

type AnchorProps = Omit<ComponentProps<"a">, "className"> & ButtonStyle;

/** 見た目はボタン、要素は <a>。外部のリンクに使う */
export const ButtonAnchor = forwardRef<HTMLAnchorElement, AnchorProps>(function ButtonAnchor(
  { variant, size, className, ...props },
  ref,
) {
  // リンクの見た目のボタンにも波紋を出す（ripple.ts）
  return (
    // oxlint-disable-next-line jsx-a11y/anchor-has-content -- 中身とラベルは children か aria-label から入る
    <a ref={ref} data-ripple className={buttonClass({ variant, size, className })} {...props} />
  );
});

/** 見た目はボタン、要素はルーターの <Link>。サイト内のページへのリンクに使う */
export const ButtonLink = createLink(ButtonAnchor);
