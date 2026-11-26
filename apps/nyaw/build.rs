// Windows の実行ファイルにアイコンリソースを埋め込む。
// ID 1 = ライトテーマ用（Explorer やショートカットが参照する既定のアイコン）、ID 2 = ダークテーマ用。
// ウィンドウ側は vendor/gpui_windows の load_icon がテーマに応じて選択する。
fn main() {
    println!("cargo:rerun-if-changed=../../assets/icons");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon_with_id("../../assets/icons/nyaw-light.ico", "1")
            .set_icon_with_id("../../assets/icons/nyaw-dark.ico", "2")
            .compile()
            .expect("failed to embed Windows icon resources");
    }
}
