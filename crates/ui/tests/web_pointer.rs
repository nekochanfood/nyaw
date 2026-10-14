//! ポインタの方式（マウス / タッチのエミュレート）の E2E を実プラットフォームの WebView で流す（モックの VR）。
//! React の Pointer test を開き、既定のマウスでは押したまま動かしてもスクロールしないこと、
//! ページのボタンでタッチにすると pointerType が touch になり、スワイプでスクロールし（pointercancel が来る）、
//! 動かさずに離すとタップ（click）になり、touch-action: none の上ではドラッグが続くことを JS 側のイベントで確かめる。
//! Linux は画面（Xvfb でよい）があれば `cargo test -p ui --test web_pointer` で流れる。

mod common;

use std::process::ExitCode;
use std::time::Duration;

use common::{PANEL_POS, Page, SharedMock, Vr, click, num, press};
use kernel::vr::Quat;
use kernel::{Kernel, Pose, Vec2, VrBackend};
use ui::PanelSpec;

fn main() -> ExitCode {
    if !common::should_run("web_pointer") {
        return ExitCode::SUCCESS;
    }
    let mock = SharedMock::default();
    // 手ブレ抑えは指した瞬間に当たらなくなるので切る（testkit と同じ）。
    let config = kernel::config::Config {
        pointer_smoothing: Some(0.0),
        ..Default::default()
    };
    let kernel = Kernel::with_backend(Box::new(mock.clone()) as Box<dyn VrBackend>, config);
    let spec = PanelSpec::web(Pose::new(PANEL_POS, Quat::IDENTITY), webpages::TESTING)
        .sized(Vec2::new(0.8, 0.6), (800, 600));
    common::run(
        "web_pointer",
        kernel,
        vec![spec],
        "panel-0",
        None,
        move |page, _| {
            scenario(page, &mut Vr::new(mock));
        },
    )
}

fn scenario(page: &mut Page, vr: &mut Vr) {
    press(page, vr, "open-pointer");
    page.until("Pointer test が開く", |s| {
        s["state"]["page"] == "pointer" && !s["state"]["status"].is_null()
    });
    assert_eq!(
        page.probe()["state"]["status"]["pointer_mode"],
        "mouse",
        "既定はマウス"
    );

    // マウス: 押したまま動かしてもスクロールしない（押して動かして離すだけ）。
    swipe_up(page, vr);
    let s = page.until("離した", |s| {
        num(s, &["state", "counts", "pointerup"]) >= 1.0
    });
    assert_eq!(s["state"]["lastPointer"]["pointerType"], "mouse");
    assert_eq!(
        num(&s, &["state", "scrollTop"]),
        0.0,
        "マウスではスクロールしない"
    );
    assert_eq!(num(&s, &["state", "counts", "pointercancel"]), 0.0);
    stick_scroll(page, vr);

    // タッチに切り替える（ページ → Tauri のコマンド → カーネル → 状態のイベント）。
    click(page, vr, "pointer-mode");
    page.until("タッチになった", |s| {
        s["state"]["status"]["pointer_mode"] == "touch"
    });

    // スワイプ: 指に付いて下へスクロールし、ポインタは取り消される。
    swipe_up(page, vr);
    let s = page.until("スワイプでスクロールした", |s| {
        num(s, &["state", "scrollTop"]) > 50.0
    });
    assert_eq!(num(&s, &["state", "counts", "pointercancel"]), 1.0);
    assert_eq!(s["state"]["lastPointer"]["pointerType"], "touch");

    // 二次元の領域も斜めのスワイプで縦横に動く。
    let (x, y) = page.center("swipe-grid");
    let downs = num(&page.probe(), &["state", "counts", "pointerdown"]);
    vr.point_at((x + 60.0, y + 80.0));
    vr.set("trigger/value", 1.0);
    page.until("二次元領域に触れた", |s| {
        num(s, &["state", "counts", "pointerdown"]) > downs
    });
    glide(vr, (x + 60.0, y + 80.0), (x - 60.0, y - 80.0));
    vr.set("trigger/value", 0.0);
    page.until("二次元のスワイプ", |s| {
        let (left, top) = scroll_position(s, "swipe-grid");
        left > 50.0 && top > 50.0
    });
    stick_scroll(page, vr);

    // タップ: 動かさずに離すと click。
    tap(page, vr, "tap-me");
    let s = page.until("タップした", |s| s["state"]["taps"] == 1);
    assert_eq!(s["state"]["lastPointer"]["pointerType"], "touch");

    // touch-action: none の上ではスクロールにならず、ドラッグで運べる。
    let (chip, zone) = (page.center("drag-chip"), page.center("drop-zone"));
    vr.point_at(chip);
    std::thread::sleep(Duration::from_millis(50));
    vr.set("trigger/value", 1.0);
    page.until("チップを押した", |s| {
        s["state"]["lastPointer"]["target"] == "drag-chip"
    });
    glide(vr, chip, zone);
    vr.set("trigger/value", 0.0);
    page.until("タッチでも運べる", |s| s["state"]["dropped"] == 1);

    // タッチのままボタンでマウスへ戻せる。
    tap(page, vr, "pointer-mode");
    page.until("マウスに戻った", |s| {
        s["state"]["status"]["pointer_mode"] == "mouse"
    });
}

fn scroll_position(s: &serde_json::Value, id: &str) -> (f64, f64) {
    if id == "swipe-list" {
        (
            num(s, &["state", "scrollLeft"]),
            num(s, &["state", "scrollTop"]),
        )
    } else {
        (
            s["state"]["scroll2d"][0].as_f64().unwrap(),
            s["state"]["scroll2d"][1].as_f64().unwrap(),
        )
    }
}

/// スティックはどちらの方式でも縦横に動き、逆方向に倒すと端まで戻れる。
fn stick_scroll(page: &mut Page, vr: &mut Vr) {
    for id in ["swipe-list", "swipe-grid"] {
        vr.point_at(page.center(id));
        let before = scroll_position(&page.probe(), id);
        vr.set("thumbstick/x", 1.0);
        vr.set("thumbstick/y", -1.0);
        let s = page.until("スティックでスクロール", |s| {
            let (left, top) = scroll_position(s, id);
            top > before.1 && (id == "swipe-list" || left > before.0)
        });
        if id == "swipe-list" {
            assert_eq!(scroll_position(&s, id).0, 0.0);
        }
        vr.set("thumbstick/x", -1.0);
        vr.set("thumbstick/y", 1.0);
        page.until("逆方向で端に戻る", |s| {
            scroll_position(s, id) == (0.0, 0.0)
        });
        vr.set("thumbstick/x", 0.0);
        vr.set("thumbstick/y", 0.0);
    }
}

/// タッチで要素の中心を押して離す。タッチは触れるまでホバーのイベントが無いので、押したことを pointerdown で見る。
fn tap(page: &mut Page, vr: &mut Vr, id: &str) {
    let downs = num(&page.probe(), &["state", "counts", "pointerdown"]);
    vr.point_at(page.center(id));
    std::thread::sleep(Duration::from_millis(50));
    vr.set("trigger/value", 1.0);
    page.until(&format!("{id} に触れた"), |s| {
        num(s, &["state", "counts", "pointerdown"]) > downs
            && s["state"]["lastPointer"]["target"] == id
    });
    vr.set("trigger/value", 0.0);
}

/// `swipe-list` の下寄りを押し、上へ 200px 少しずつ動かして離す。
fn swipe_up(page: &mut Page, vr: &mut Vr) {
    let (x, y) = page.center("swipe-list");
    let ups = num(&page.probe(), &["state", "counts", "pointerdown"]);
    vr.point_at((x, y + 100.0));
    std::thread::sleep(Duration::from_millis(50));
    vr.set("trigger/value", 1.0);
    page.until("一覧を押した", |s| {
        num(s, &["state", "counts", "pointerdown"]) > ups
    });
    glide(vr, (x, y + 100.0), (x, y - 100.0));
    vr.set("trigger/value", 0.0);
}

/// レーザーを `from` から `to` へ少しずつ動かす。
fn glide(vr: &mut Vr, from: (f32, f32), to: (f32, f32)) {
    for i in 1..=10 {
        let t = i as f32 / 10.0;
        vr.point_at((from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t));
        std::thread::sleep(Duration::from_millis(15));
    }
}
