fn main() {
    // Windows: 本体（nyaw）は gpui と同じ exe に入る。
    // - マニフェストは gpui も埋め込む（Common Controls v6 を含む）ので、Tauri の分を入れると重複してリンクできない。
    // - 静的 CRT はリンク引数が webui 自身にしか付かないのに、差し替え用の空の msvcrt.lib の置き場は依存先へ漏れ、
    //   webui に依存するクレート（webtesting 等）のテストが CRT を失ってリンクできない。Rust の既定（動的 CRT）に揃える。
    // 以前のビルドが置いた空の msvcrt.lib が OUT_DIR に残っていると同じことになるので消す。
    let stale =
        std::path::Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).join("msvcrt.lib");
    let _ = std::fs::remove_file(stale);
    let windows =
        tauri_build::WindowsAttributes::new_without_app_manifest().static_vc_runtime(false);
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("tauri-build");
    // gpui の入らない webui 自身のテスト（WebView の E2E）には、Tauri が要る Common Controls v6 をリンカで付ける。
    // 無いと TaskDialogIndirect が解決できず起動できない。
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-tests=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
}
