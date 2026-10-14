fn main() {
    // Windows: 本体（nyaw）は gpui と同じ exe に入る。
    // - マニフェストは gpui も埋め込む（Common Controls v6 を含む）ので、Tauri の分を入れると重複してリンクできない。
    // - 空の msvcrt.lib が依存先の CRT を隠すため、Rust の既定（動的 CRT）に揃える。
    // 以前のビルドが置いた空の msvcrt.lib が OUT_DIR に残っていると同じことになるので消す。
    let stale =
        std::path::Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).join("msvcrt.lib");
    let _ = std::fs::remove_file(stale);
    let windows =
        tauri_build::WindowsAttributes::new_without_app_manifest().static_vc_runtime(false);
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("tauri-build");
}
