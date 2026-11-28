import { expect, test } from "@playwright/test";

import { gotoStill, watchErrors } from "./helpers";

test("テーマの切り替えを保存し、読み込み直しても保つ", async ({ page }) => {
  const assertNoErrors = watchErrors(page);
  await gotoStill(page, "/en");
  const html = page.locator("html");
  await page.getByRole("button", { name: "Toggle theme" }).click();
  await expect(html).toHaveAttribute("data-theme", "dark");
  // 切り替える瞬間だけ付く、トランジションを止める印は残らない
  await expect(html).not.toHaveAttribute("data-theme-switching");
  expect(await page.evaluate(() => localStorage.getItem("nyaw-theme"))).toBe("dark");
  await page.reload();
  await expect(html).toHaveAttribute("data-theme", "dark");
  assertNoErrors();
});

test("言語メニューで言語を替え、cookie に残し、メニューは閉じる", async ({
  page,
  context,
  isMobile,
}) => {
  test.skip(isMobile, "言語メニューの操作はデスクトップで確かめる");
  await gotoStill(page, "/en/features");
  await page.getByRole("button", { name: "Change language" }).click();
  const menu = page.getByRole("menu");
  await expect(menu).toBeVisible();
  await page.getByRole("menuitemradio", { name: "日本語" }).click();
  await expect(page).toHaveURL(/\/ja\/features$/);
  await expect(page.locator("html")).toHaveAttribute("lang", "ja");
  // 退場の動きのために引き止めたポップアップが、終わったあとに外れる
  await expect(menu).toHaveCount(0);
  const cookie = (await context.cookies()).find((c) => c.name === "locale");
  expect(cookie?.value).toBe("ja");
});

test("狭い画面のメニューからページへ移り、メニューは閉じる", async ({ page, isMobile }) => {
  test.skip(!isMobile, "メニューのボタンは狭い画面だけに出る");
  await gotoStill(page, "/en");
  await page.getByRole("button", { name: "Open menu" }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByRole("link", { name: "Download" }).click();
  await expect(page).toHaveURL(/\/en\/download$/);
  await expect(dialog).toHaveCount(0);
});

test("動きがあるときも、読み込み画面、ページの入れ替えの順に進み、最後はページが見える", async ({
  page,
}) => {
  const assertNoErrors = watchErrors(page);
  await page.goto("/en");
  await expect(page.locator("#loading-screen")).toBeHidden();
  await expect(page.locator("[data-page]")).toHaveCSS("opacity", "1");
  // フッターのリンクはどの幅でも出る
  await page.getByRole("contentinfo").getByRole("link", { name: "Download" }).click();
  await expect(page).toHaveURL(/\/en\/download$/);
  await expect(page.locator("[data-page]")).toHaveCSS("opacity", "1");
  assertNoErrors();
});

test("動きがあるときに言語を替えても、ヘッダーのリンクは見えたまま", async ({ page, isMobile }) => {
  test.skip(isMobile, "ナビゲーションのリンクはデスクトップの幅だけに出る");
  await page.goto("/en");
  const features = page.getByRole("banner").getByRole("link", { name: "Features" });
  await expect(features).toHaveCSS("opacity", "1");
  await page.getByRole("button", { name: "Change language" }).click();
  await page.getByRole("menuitemradio", { name: "日本語" }).click();
  await expect(page).toHaveURL(/\/ja$/);
  await expect(page.getByRole("banner").getByRole("link", { name: "機能" })).toHaveCSS(
    "opacity",
    "1",
  );
});
