//! novr（`Surface::Desktop`）の E2E。本番と同じ `ui::run_with` で GPUI の Testing を開き、そこから Testing の Web 版を開く
//! （ボタンと同じく `PanelControl::spawns` に `PanelSpec::spawn_web` を積む。ボタン自体は testkit の `open_web_testing`）。
//! WebView は OS のウィンドウとして表示され、OS のマウスとキーボード（xdotool で XTest 入力を送る）がそのまま
//! React / Base UI に届く。ページのボタン → Tauri のコマンド → カーネル、OS の操作でのウィンドウのリサイズ → パネルの解像度も確かめる。
//! Linux の X11（Xvfb でよい）で xdotool があれば `cargo test -p ui --test web_novr` で流れる。

mod common;

use std::process::{Command, ExitCode};
use std::time::Duration;

use common::{Page, num};
use kernel::{Kernel, Pose, Surface};
use serde_json::{Value, json};
use tauri::Manager;
use ui::{PanelControl, PanelSpec};

fn main() -> ExitCode {
    if !common::should_run("web_novr") {
        return ExitCode::SUCCESS;
    }
    if !cfg!(target_os = "linux") || Command::new("xdotool").arg("version").output().is_err() {
        println!("web_novr: スキップ（xdotool が無い）");
        return ExitCode::SUCCESS;
    }
    let kernel = Kernel::boot(Some("novr"), Default::default()).expect("novr は常に起動できる");
    assert_eq!(kernel.surface(), Surface::Desktop);
    // GPUI の Testing（パネル 0）から、Web 版（パネル 1）を開く。
    let gpui = PanelSpec::new(Pose::IDENTITY, |id, _, _| testing::Testing::new(id));
    let open_web: ui::Remote = Box::new(|cx| {
        cx.default_global::<PanelControl>()
            .spawns
            .push(PanelSpec::spawn_web(0, testing::WEB_PAGE))
    });
    common::run(
        "web_novr",
        kernel,
        vec![gpui],
        "panel-1",
        Some(open_web),
        |page, _| scenario(page),
    )
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

fn has(list: &Value, id: &str) -> bool {
    list.as_array().is_some_and(|a| a.iter().any(|v| v == id))
}

fn scenario(page: &mut Page) {
    // GPUI の Testing から開いた Web のパネル（パネル 1）。GPUI の Testing（パネル 0）も同じカーネルで開いている。
    let s = page.probe();
    assert_eq!(s["state"]["status"]["panel"], 1);
    assert_eq!(s["state"]["status"]["runtime"], "novr");

    // ホバー: OS のマウスがそのまま（isTrusted な）ポインタイベントになり、CSS の :hover も効く。
    move_to(page, "click-me");
    let s = page.until("マウスが乗る", |s| s["state"]["hovered"] == true);
    let last = &s["state"]["lastPointer"];
    assert_eq!(last["pointerType"], "mouse");
    assert_eq!(last["trusted"], true, "OS のネイティブ入力");
    assert!(
        s["hover"].as_array().is_some_and(|a| a.is_empty()),
        "novr ではブリッジの合成は使わない"
    );

    // クリック。
    xdotool(&["click", "1"]);
    let s = page.until("クリックした", |s| s["state"]["clicks"] == 1);
    for t in ["pointerdown", "pointerup", "click"] {
        assert_eq!(num(&s, &["state", "counts", t]), 1.0, "{t}");
    }

    // Base UI のコンポーネント。
    click(page, "toggle");
    page.until("Toggle", |s| s["state"]["toggled"] == true);
    click(page, "switch");
    page.until("Switch", |s| s["state"]["switched"] == true);
    click(page, "checkbox");
    page.until("Checkbox", |s| s["state"]["checked"] == true);
    click(page, "tab-b");
    page.until("Tabs", |s| s["state"]["tab"] == "tab-b");
    click(page, "open-menu");
    page.until("メニューが開いた", |s| {
        s["rects"]["menu-a"].is_object()
    });
    click(page, "menu-a");
    page.until("選んで閉じた", |s| {
        s["state"]["picked"] == "menu-a" && s["state"]["menuOpen"] == false
    });

    // Slider: つまみを右端まで引く。
    move_to(page, "slider-thumb");
    let thumb = page.center("slider-thumb");
    let r = page.probe()["rects"]["slider"].clone();
    let right = (r["x"].as_f64().unwrap() + r["w"].as_f64().unwrap()) as f32 + 20.0;
    xdotool(&["mousedown", "1"]);
    for i in 1..=8 {
        let at = screen(
            page,
            (thumb.0 + (right - thumb.0) * i as f32 / 8.0, thumb.1),
        );
        xdotool(&["mousemove", &at[0], &at[1]]);
    }
    xdotool(&["mouseup", "1"]);
    page.until("右端まで動いた", |s| {
        num(s, &["state", "slider"]) == 100.0
    });

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

    // フォーカスとキーボード（novr だけ）。
    click(page, "text-field");
    page.until("入力欄にフォーカス", |s| {
        s["focused"] == "text-field"
    });
    xdotool(&["type", "--delay", "30", "hi nyaw"]);
    page.until("文字が入った", |s| s["state"]["text"] == "hi nyaw");

    // カーネルとの対話はどのランタイムでも同じ。
    click(page, "lock");
    page.until("ロックした", |s| {
        s["state"]["status"]["locked"] == true
    });

    // リサイズ（コマンド）: ビューポートとカーネルの解像度がそろって変わる。
    click(page, "resize-large");
    page.until("大きくなった", |s| {
        s["viewport"] == json!([1000, 700])
            && s["state"]["status"]["resolution"] == json!([1000, 700])
    });

    // リサイズ（OS の操作）: ウィンドウを OS 側で変えても、ページとカーネルが追従する。
    let out = Command::new("xdotool")
        .args(["search", "--onlyvisible", "--name", "^testing$"])
        .output()
        .expect("xdotool search");
    let ids = String::from_utf8_lossy(&out.stdout);
    let id = ids
        .lines()
        .last()
        .expect("ウィンドウが見つからない")
        .to_owned();
    xdotool(&["windowsize", &id, "700", "500"]);
    let s = page.until("OS で小さくした", |s| {
        s["viewport"] == json!([700, 500])
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

    // 半透明のページ（novr では普通のウィンドウ）も開いて操作できる。
    click(page, "open-transparent");
    let deadline = std::time::Instant::now() + common::TIMEOUT;
    while page.app.get_webview_window("panel-2").is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "半透明のページが開かない"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    page.label = "panel-2".into();
    page.until("半透明のページが読み込まれる", |s| {
        s["state"]["clicks"] == 0
    });
    click(page, "glass-button");
    page.until("半透明のボタンを押した", |s| {
        s["state"]["clicks"] == 1
    });
    click(page, "close");
    let deadline = std::time::Instant::now() + common::TIMEOUT;
    while page.app.get_webview_window("panel-2").is_some() {
        assert!(std::time::Instant::now() < deadline, "閉じない");
        std::thread::sleep(Duration::from_millis(20));
    }
    page.label = "panel-1".into();
    assert!(!has(&page.probe()["hover"], "close"));
}
