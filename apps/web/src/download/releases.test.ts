import { describe, expect, it } from "vitest";

import { type GitHubRelease, megabytes, pickReleases } from "@/download/releases";

const release = (
  tag: string,
  published: string,
  extra: Partial<GitHubRelease> = {},
): GitHubRelease => ({
  tag_name: tag,
  draft: false,
  prerelease: tag.includes("-"),
  published_at: published,
  html_url: `https://github.com/nekochanfood/nyaw/releases/tag/${tag}`,
  assets: [
    {
      name: `nyaw-${tag.slice(1)}-setup.exe`,
      size: 134_376_614,
      browser_download_url: `https://example.com/${tag}.exe`,
    },
  ],
  ...extra,
});

describe("pickReleases", () => {
  it("最新の正式版を選び、それより古いプレビュー版は出さない", () => {
    const { stable, preview } = pickReleases([
      release("v2026.0.4-beta.4", "2026-11-27T07:00:00Z"),
      release("v2026.0.4", "2026-11-27T13:00:00Z"),
      release("v2026.0.3", "2026-11-20T00:00:00Z"),
    ]);
    expect(stable?.version).toBe("2026.0.4");
    expect(stable?.installer.url).toBe("https://example.com/v2026.0.4.exe");
    expect(preview).toBeUndefined();
  });

  it("正式版より新しいプレビュー版を選ぶ。ラベルは何でもよい", () => {
    const { stable, preview } = pickReleases([
      release("v2026.0.4", "2026-11-27T13:00:00Z"),
      release("v2026.0.5-potato.2", "2026-11-29T00:00:00Z"),
      release("v2026.0.5-potato.1", "2026-11-28T00:00:00Z"),
    ]);
    expect(stable?.version).toBe("2026.0.4");
    expect(preview?.version).toBe("2026.0.5-potato.2");
  });

  it("下書きとインストーラーのないリリースは飛ばす", () => {
    const { stable } = pickReleases([
      release("v2026.0.6", "2026-12-02T00:00:00Z", { draft: true }),
      release("v2026.0.5", "2026-12-01T00:00:00Z", { assets: [] }),
      release("v2026.0.4", "2026-11-27T13:00:00Z"),
    ]);
    expect(stable?.version).toBe("2026.0.4");
  });

  it("正式版がなければプレビュー版だけを返す", () => {
    const { stable, preview } = pickReleases([release("v2026.0.1-beta.1", "2026-11-01T00:00:00Z")]);
    expect(stable).toBeUndefined();
    expect(preview?.version).toBe("2026.0.1-beta.1");
  });

  it("空の一覧では何も返さない", () => {
    expect(pickReleases([])).toEqual({ stable: undefined, preview: undefined });
  });
});

describe("megabytes", () => {
  it("MB で小数 1 桁にする", () => {
    expect(megabytes(134_376_614)).toBe("128.2");
  });
});
