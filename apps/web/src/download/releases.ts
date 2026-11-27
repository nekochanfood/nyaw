/** GitHub の API が返すリリース（使う項目だけ） */
export type GitHubRelease = {
  tag_name: string;
  draft: boolean;
  prerelease: boolean;
  published_at: string | null;
  html_url: string;
  assets: { name: string; size: number; browser_download_url: string }[];
};

export type Release = {
  /** 先頭の v を除いた版（2026.0.4、2026.0.5-beta.1） */
  version: string;
  published: Date;
  page: string;
  installer: { url: string; size: number };
};

export const RELEASES_API = "https://api.github.com/repos/nekochanfood/nyaw/releases?per_page=20";

/** インストーラーのある公開済みのリリースだけを、公開の新しい順に並べる */
function usable(releases: GitHubRelease[]) {
  return releases
    .flatMap((r) => {
      const installer = r.assets.find((a) => /^nyaw-.+-setup\.exe$/.test(a.name));
      if (r.draft || !r.published_at || !installer) return [];
      return [{ release: r, installer, published: new Date(r.published_at) }];
    })
    .sort((a, b) => b.published.getTime() - a.published.getTime());
}

/**
 * 最新の正式版と、それより新しいナイトリー（あれば）を選ぶ。
 * 版の新旧はアプリと同じく版の文字列では比べられない（ラベルが自由）ので、公開日時で比べる。
 */
export function pickReleases(releases: GitHubRelease[]) {
  const list = usable(releases);
  const toRelease = ({ release, installer, published }: (typeof list)[number]): Release => ({
    version: release.tag_name.replace(/^v/, ""),
    published,
    page: release.html_url,
    installer: { url: installer.browser_download_url, size: installer.size },
  });
  const stable = list.find((r) => !r.release.prerelease);
  const nightly = list.find(
    (r) => r.release.prerelease && (!stable || r.published > stable.published),
  );
  return {
    stable: stable ? toRelease(stable) : undefined,
    nightly: nightly ? toRelease(nightly) : undefined,
  };
}

/** バイト数を MB で、小数 1 桁まで */
export const megabytes = (bytes: number) => (bytes / 1024 / 1024).toFixed(1);
