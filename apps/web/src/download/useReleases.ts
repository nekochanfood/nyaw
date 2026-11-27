import { useEffect, useState } from "react";

import { type GitHubRelease, pickReleases, RELEASES_API } from "@/download/releases";

type State =
  | { status: "loading" }
  | { status: "error" }
  | ({ status: "ready" } & ReturnType<typeof pickReleases>);

/**
 * GitHub のリリースをブラウザから取得する。API の回数制限は訪問者ごとに数えられるので、サーバーでまとめて
 * 取得するより制限に当たりにくい。取得できなければ GitHub Releases へ案内する。
 */
export function useReleases(): State {
  const [state, setState] = useState<State>({ status: "loading" });
  useEffect(() => {
    const controller = new AbortController();
    fetch(RELEASES_API, {
      signal: controller.signal,
      headers: { accept: "application/vnd.github+json" },
    })
      .then((res) => {
        if (!res.ok) throw new Error(`GitHub API: ${res.status}`);
        return res.json() as Promise<GitHubRelease[]>;
      })
      .then((releases) => setState({ status: "ready", ...pickReleases(releases) }))
      .catch(() => {
        if (!controller.signal.aborted) setState({ status: "error" });
      });
    return () => controller.abort();
  }, []);
  return state;
}
