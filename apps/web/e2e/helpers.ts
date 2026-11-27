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

const asset = (tag: string, size = 134_376_614) => ({
  name: `nyaw-${tag.slice(1)}-setup.exe`,
  size,
  browser_download_url: `https://github.com/nekochanfood/nyaw/releases/download/${tag}/nyaw-${tag.slice(1)}-setup.exe`,
});

const release = (tag: string, published: string) => ({
  tag_name: tag,
  draft: false,
  prerelease: tag.includes("-"),
  published_at: published,
  html_url: `https://github.com/nekochanfood/nyaw/releases/tag/${tag}`,
  assets: [asset(tag)],
});

/** GitHub のリリース一覧を、決まった内容（安定版と、それより新しいナイトリー）に差し替える */
export async function mockReleases(page: Page, status = 200) {
  await page.route("https://api.github.com/repos/nekochanfood/nyaw/releases**", (route) =>
    route.fulfill({
      status,
      json:
        status === 200
          ? [
              release("v2026.0.5-beta.1", "2026-11-28T00:00:00Z"),
              release("v2026.0.4", "2026-11-27T13:00:00Z"),
            ]
          : { message: "rate limited" },
    }),
  );
}

/** 開いたページで、起動の動きとスクロールで現れる要素の動きが終わるのを待たずに読めるようにする */
export async function gotoStill(page: Page, path: string) {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto(path);
}
