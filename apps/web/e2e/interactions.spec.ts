import { expect, test } from "@playwright/test";

import { gotoStill, mockReleases, watchErrors } from "./helpers";

test("テーマの切り替えを保存し、読み込み直しても保つ", async ({ page }) => {
  const assertNoErrors = watchErrors(page);
  await gotoStill(page, "/en");
  const html = page.locator("html");
  await page.getByRole("button", { name: "Toggle theme" }).click();
  await expect(html).toHaveAttribute("data-theme", "dark");
  // 切り替えの間だけ付く、色のトランジションの印は外れる
  await expect(html).not.toHaveAttribute("data-theme-fading");
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
  await mockReleases(page);
  await gotoStill(page, "/en");
  await page.getByRole("button", { name: "Open menu" }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByRole("link", { name: "Download" }).click();
  await expect(page).toHaveURL(/\/en\/download$/);
  await expect(dialog).toHaveCount(0);
});

test.describe("ダウンロード", () => {
  test("安定版とナイトリーを切り替える", async ({ page }) => {
    await mockReleases(page);
    await gotoStill(page, "/en/download");
    const version = page.getByTestId("version").filter({ visible: true });
    await expect(version).toHaveText("2026.0.4");
    await expect(
      page.getByRole("link", { name: /Download installer/ }).filter({ visible: true }),
    ).toHaveAttribute("href", /v2026\.0\.4\/nyaw-2026\.0\.4-setup\.exe$/);
    await page.getByRole("tab", { name: "Nightly" }).click();
    await expect(version).toHaveText("2026.0.5-beta.1");
    await expect(page.getByText("27 November 2026")).toHaveCount(0);
    await expect(page.getByText("Released 28 November 2026")).toBeVisible();
  });

  test("リリースを取得できなければ GitHub Releases へ案内する", async ({ page }) => {
    await mockReleases(page, 403);
    await gotoStill(page, "/ja/download");
    await expect(page.getByText("最新版を確認できませんでした")).toBeVisible();
    await expect(page.getByRole("link", { name: "GitHub Releases を開く" })).toHaveAttribute(
      "href",
      "https://github.com/nekochanfood/nyaw/releases",
    );
  });
});

test("動きを減らす設定でも、スクロールで現れる要素は最初から見える", async ({ page }) => {
  await gotoStill(page, "/en/features");
  const card = page.getByRole("heading", { name: "Virtual keyboard" });
  await card.scrollIntoViewIfNeeded();
  await expect(card).toBeVisible();
  expect(await card.evaluate((el) => getComputedStyle(el.closest("li")!).opacity)).toBe("1");
});
