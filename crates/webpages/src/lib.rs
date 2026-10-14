//! Web UI のページ（リポジトリ直下の `web/`。React + Tailwind + Base UI）。build.rs が Vite でビルドして埋め込む。
//! UI 層はこれを `nyaw://` で配信し、`PanelSpec::web("<ページ名>")` で開く。どのページかは UI 層は知らない。

include!(concat!(env!("OUT_DIR"), "/assets.rs"));

/// Testing の Web 版。基本的なコンポーネント（Base UI）と VR（カーネル）の操作を試す。
pub const TESTING: &str = "testing";
/// 半透明の確認用（背景が透明で、不透明度の違う要素を並べる）。
pub const TESTING_TRANSPARENT: &str = "testing-transparent";
/// マウス／タッチのエミュレートと一次元・二次元スクロールを試す。
pub const TESTING_POINTER: &str = "testing-pointer";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_has_an_entry_and_shares_assets() {
        if FILES.is_empty() {
            return; // NYAW_WEB_SKIP
        }
        for page in [TESTING, TESTING_TRANSPARENT, TESTING_POINTER] {
            let entry = format!("{page}/index.html");
            let (_, html) = FILES
                .iter()
                .find(|(p, _)| *p == entry)
                .unwrap_or_else(|| panic!("{entry} が無い"));
            let html = std::str::from_utf8(html).unwrap();
            // 共有の assets/ を相対パスで読む（nyaw://localhost/<ページ>/ から辿れる）。
            assert!(html.contains("../assets/"), "{entry}: {html}");
        }
        assert!(
            FILES
                .iter()
                .any(|(p, _)| p.starts_with("assets/") && p.ends_with(".js"))
        );
    }
}
