fn main() {
    // Windows: 本体（nyaw）は gpui と同じ exe に入る。
    // - マニフェストは gpui も埋め込む（Common Controls v6 を含む）ので、Tauri の分を入れると重複してリンクできない。
    // - 静的 CRT はリンク引数が webui 自身にしか付かないのに、差し替え用の空の msvcrt.lib の置き場は依存先へ漏れ、
    //   webui に依存するクレート（webtesting 等）のテストが CRT を失ってリンクできない。Rust の既定（動的 CRT）に揃える。
    let windows =
        tauri_build::WindowsAttributes::new_without_app_manifest().static_vc_runtime(false);
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("tauri-build");
}
