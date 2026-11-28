import { expect, type Page } from "@playwright/test";

/** ページの例外とコンソールのエラーを集め、呼び出したときに何もなかったことを確かめる */
export function watchErrors(page: Page) {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("console", (m) => {
    if (m.type() === "error") errors.push(m.text());
  });
  return () => expect(errors).toEqual([]);
}

/**
 * 動きを減らす設定でページを開き、操作を受け付けられるようになるまで待つ。
 * 起動の動きやスクロールで現れる要素の動きを待たずに読め、ハイドレーションの前に押して空振りすることもない。
 */
export async function gotoStill(page: Page, path: string) {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto(path);
  await page.locator("html[data-hydrated]").waitFor({ state: "attached" });
}
