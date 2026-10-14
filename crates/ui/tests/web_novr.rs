//! novr（`Surface::Desktop`）の E2E を実プラットフォームの WebView で流す。WebView は OS のウィンドウとして表示され、
//! OS のマウス（xdotool で XTest 入力を送る）がそのまま JS のポインタイベントになる。ページのボタン → Tauri のコマンド →
//! カーネル、OS の操作でのウィンドウのリサイズ → パネルの解像度も確かめる。
//! Linux の X11（Xvfb でよい）で xdotool があれば `cargo test -p webui --test webview_novr` で流れる。

mod common;

use std::process::{Command, ExitCode};
use std::time::Duration;

use common::{Page, num};
use kernel::{Kernel, Surface};
use serde_json::json;
use tauri::Manager;

fn main() -> ExitCode {
    if !common::should_run("webview_novr") {
        return ExitCode::SUCCESS;
    }
    if !cfg!(target_os = "linux") || Command::new("xdotool").arg("version").output().is_err() {
        println!("webview_novr: スキップ（xdotool が無い）");
        return ExitCode::SUCCESS;
    }
    let kernel = Kernel::boot(Some("novr"), Default::default()).expect("novr は常に起動できる");
    assert_eq!(kernel.surface(), Surface::Desktop);
    common::run("webview_novr", kernel, vec![webtesting::spec()], scenario)
}

fn xdotool(args: &[&str]) {
    let ok = Command::new("xdotool")
        .args(args)
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok, "xdotool {args:?} に失敗");
    std::thread::sleep(Duration::from_millis(30));
}

/// ページ上の CSS px を画面の座標にする（拡大率 1 の X11 前提）。
fn screen(page: &Page, (x, y): (f32, f32)) -> [String; 2] {
    let w = page.app.get_webview_window(&page.label).unwrap();
    let p = w.inner_position().unwrap();
    [
        (p.x + x.round() as i32).to_string(),
        (p.y + y.round() as i32).to_string(),
    ]
}

fn move_to(page: &mut Page, id: &str) {
    let c = page.center(id);
    let at = screen(page, c);
    xdotool(&["mousemove", &at[0], &at[1]]);
}

fn click(page: &mut Page, id: &str) {
    move_to(page, id);
    xdotool(&["click", "1"]);
}

fn scenario(page: &mut Page) {
    // ホバー: OS のマウスがそのまま（isTrusted な）ポインタイベントになる。
    move_to(page, "click-me");
    let s = page.until("マウスが乗る", |s| s["state"]["hovered"] == true);
    let last = &s["state"]["lastPointer"];
    assert_eq!(last["pointerType"], "mouse");
    assert_eq!(last["trusted"], true, "OS のネイティブ入力");

    // クリック。
    xdotool(&["click", "1"]);
    let s = page.until("クリックした", |s| s["state"]["clicks"] == 1);
    for t in ["pointerdown", "pointerup", "click"] {
        assert_eq!(num(&s, &["state", "counts", t]), 1.0, "{t}");
    }

    // ドラッグ: ネイティブのポインタキャプチャで運んで落とす。
    move_to(page, "drag-chip");
    let zone = page.center("drop-zone");
    let chip = page.center("drag-chip");
    xdotool(&["mousedown", "1"]);
    for i in 1..=8 {
        let t = i as f32 / 8.0;
        let at = screen(
            page,
            (
                chip.0 + (zone.0 - chip.0) * t,
                chip.1 + (zone.1 - chip.1) * t,
            ),
        );
        xdotool(&["mousemove", &at[0], &at[1]]);
        if i == 1 {
            // gotpointercapture は setPointerCapture の次のポインタイベントで届く（仕様どおり）。
            page.until("キャプチャした", |s| s["state"]["captured"] == true);
        }
    }
    xdotool(&["mouseup", "1"]);
    page.until("落とした", |s| s["state"]["dropped"] == true);

    // スクロール: ホイールを下へ。
    move_to(page, "scroll-list");
    xdotool(&["click", "5"]);
    xdotool(&["click", "5"]);
    page.until("スクロールした", |s| {
        num(s, &["state", "wheel"]) > 0.0 && num(s, &["state", "scrollTop"]) > 0.0
    });

    // フォーカス。
    click(page, "text-field");
    page.until("入力欄にフォーカス", |s| {
        s["focused"] == "text-field"
    });

    // カーネルとの対話はどのランタイムでも同じ。
    click(page, "lock");
    let s = page.until("ロックした", |s| {
        s["state"]["status"]["locked"] == true
    });
    assert_eq!(s["state"]["status"]["runtime"], "novr");

    // リサイズ（コマンド）: ビューポートとカーネルの解像度がそろって変わる。
    click(page, "resize-large");
    page.until("大きくなった", |s| {
        s["state"]["viewport"] == json!([1000, 700])
            && s["state"]["status"]["resolution"] == json!([1000, 700])
    });

    // リサイズ（OS の操作）: ウィンドウを OS 側で変えても、ページとカーネルが追従する。
    let out = Command::new("xdotool")
        .args([
            "search",
            "--onlyvisible",
            "--name",
            "Testing \\(Web\\)|webtesting",
        ])
        .output()
        .expect("xdotool search");
    let ids = String::from_utf8_lossy(&out.stdout);
    let id = ids
        .lines()
        .next()
        .expect("ウィンドウが見つからない")
        .to_owned();
    xdotool(&["windowsize", &id, "700", "500"]);
    let s = page.until("OS で小さくした", |s| {
        s["state"]["viewport"] == json!([700, 500])
            && s["state"]["status"]["resolution"] == json!([700, 500])
    });
    let width_m = s["state"]["status"]["size"][0].as_f64().unwrap();
    assert!(
        (width_m - 0.7).abs() < 1e-4,
        "1m あたりのピクセル数を保つ: {width_m}"
    );

    // 新しいレイアウトでもクリックできる。
    click(page, "click-me");
    page.until("リサイズ後もクリックできる", |s| {
        s["state"]["clicks"] == 2
    });
}
