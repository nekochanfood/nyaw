//! novr（エミュレーター）の E2E。本番と同じ `ui::run_with` で GPUI の Testing を開き、そこから Testing の Web 版を開く
//! （ボタンと同じく `PanelControl::spawns` に `PanelSpec::spawn_web` を積む。ボタン自体は testkit の `open_web_testing`）。
//! 本番の novr と同じく WebView は画面に出さずに描いて読み戻し、エミュレーターの画面に合成される。入力はエミュレーターの
//! マウス（右手のコントローラ）: 左クリック = トリガー、右クリック = グリップ、矢印キー = スティック。ページへはブリッジの合成イベントで届く。
//! エミュレーターはウィンドウを出さない（headless）で、テストのスレッドから `common::Emu` で操作する。
//! Linux は画面（Xvfb でよい）があれば `cargo test -p ui --test web_novr` で流れる。

mod common;

use std::process::ExitCode;

use common::{Driven, Emu, Page, num};
use kernel::vr::novr::Button;
use kernel::vr::{Quat, Vec3};
use kernel::{Kernel, Pose, Vec2, VrBackend};
use serde_json::Value;
use ui::{PanelControl, PanelSpec};

/// GPUI の Testing（パネル 0）の位置。Web 版（パネル 1）はその中心の手前 5cm に重ねて開く。
const GPUI_POS: Vec3 = Vec3::new(-0.6, 0.0, -1.5);
const WEB_POS: Vec3 = Vec3::new(-0.6, 0.0, -1.45);

/// Web のパネル上の点（CSS px）が映る画面上の点。大きさ・解像度はページが受け取った状態から読む。
fn on_screen(emu: &Emu, status: &Value, (x, y): (f32, f32)) -> Vec2 {
    let f = |v: &Value| v.as_f64().unwrap() as f32;
    let (sw, sh) = (f(&status["size"][0]), f(&status["size"][1]));
    let (rw, rh) = (f(&status["resolution"][0]), f(&status["resolution"][1]));
    let p = WEB_POS + Vec3::new((x / rw - 0.5) * sw, (0.5 - y / rh) * sh, 0.0);
    emu.run(move |e| e.project(p)).expect("パネルは目の前")
}

fn near(rgb: [u8; 3]) -> impl Fn([u8; 3]) -> bool {
    move |c| {
        c.iter()
            .zip(rgb)
            .all(|(a, b)| (*a as i32 - b as i32).abs() <= 8)
    }
}

fn gray(c: [u8; 3]) -> bool {
    c[0] == c[1] && c[1] == c[2]
}

fn main() -> ExitCode {
    if !common::should_run("web_novr") {
        return ExitCode::SUCCESS;
    }
    let (emu, tx) = Driven::headless(1280, 720);
    // 手ブレ抑えは指した瞬間に当たらなくなるので切る（testkit と同じ）。
    let config = kernel::config::Config {
        pointer_smoothing: Some(0.0),
        // 操作のシナリオはマウスで書く（既定のタッチは web_pointer で確かめる）。
        pointer_mode: Some(kernel::PointerMode::Mouse),
        ..Default::default()
    };
    let kernel = Kernel::with_backend(Box::new(emu) as Box<dyn VrBackend>, config);
    // GPUI の Testing（パネル 0）から、Web 版（パネル 1）を開く。
    let gpui = PanelSpec::new(Pose::new(GPUI_POS, Quat::IDENTITY), |id, _, _| {
        testing::Testing::new(id)
    });
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
        move |page, _| scenario(page, &tx),
    )
}

/// 要素の中心をマウスで指す。
fn hover(page: &mut Page, emu: &Emu, id: &str) -> Vec2 {
    let status = page.probe()["state"]["status"].clone();
    let at = on_screen(emu, &status, page.center(id));
    emu.point(Some(at));
    page.until(&format!("{id} に乗る"), |s| {
        s["state"]["last"]["target"] == id
    });
    at
}

/// 要素の中心を左クリック（トリガー）する。
fn click(page: &mut Page, emu: &Emu, id: &str) {
    hover(page, emu, id);
    let downs = num(&page.probe(), &["state", "counts", "pointerdown"]);
    emu.button(Button::Trigger, true);
    page.until(&format!("{id} を押した"), |s| {
        num(s, &["state", "counts", "pointerdown"]) > downs
    });
    emu.button(Button::Trigger, false);
}

fn scenario(page: &mut Page, emu: &Emu) {
    let s = page.probe();
    assert_eq!(s["state"]["status"]["panel"], 1);
    assert_eq!(s["state"]["status"]["runtime"], "novr");
    let status = s["state"]["status"].clone();
    let h = status["resolution"][1].as_f64().expect("解像度") as f32;

    // 画面に出さない WebView が読み戻されて、エミュレーターの画面に映る（背景は不透明な #202020）。
    // パネルの外（右）は無彩色の市松模様。
    let w = status["resolution"][0].as_f64().expect("解像度") as f32;
    let bottom_left = on_screen(emu, &status, (4.0, h - 10.0));
    emu.until_color(
        "ページの描画が画面に映る",
        bottom_left,
        near([0x20, 0x20, 0x20]),
    );
    let outside = on_screen(emu, &status, (w - 4.0, h - 10.0)) + Vec2::new(100.0, 0.0);
    assert!(gray(emu.color(outside)), "パネルの外は無彩色");
    assert_ne!(
        emu.color(Vec2::new(0.0, 0.0)),
        emu.color(Vec2::new(16.0, 0.0)),
        "市松模様"
    );

    // ホバー: マウスの方向が右手のレイになり、ブリッジの合成イベントで届く（Tailwind の hover: の色まで映る）。
    hover(page, emu, "click-me");
    let s = page.until("マウスが乗る", |s| s["state"]["hovered"] == true);
    assert_eq!(
        s["state"]["lastPointer"]["trusted"], false,
        "ブリッジが合成したイベント"
    );
    let edge = on_screen(emu, &status, page.left_edge("click-me", 3.0));
    emu.until_color("ホバーの色が画面に映る", edge, near([0x50, 0x54, 0x5f]));

    // 左クリック = トリガー。
    emu.button(Button::Trigger, true);
    page.until("押した", |s| {
        num(s, &["state", "counts", "pointerdown"]) == 1.0
    });
    emu.button(Button::Trigger, false);
    page.until("クリックした", |s| s["state"]["clicks"] == 1);
    emu.until_color("クリックの結果が画面に映る", edge, near([0x2e, 0xcc, 0x71]));

    // 矢印キー = 右手のスティック。下へ倒すと一覧が下へ動く。
    // 一覧の中心には行があるので、宛先が一覧そのものになるのは待たない。
    let list = on_screen(emu, &status, page.center("scroll-list"));
    emu.point(Some(list));
    emu.stick(Vec2::new(0.0, -1.0));
    page.until("スクロールした", |s| {
        num(s, &["state", "wheel"]) > 0.0 && num(s, &["state", "scrollTop"]) > 0.0
    });
    emu.stick(Vec2::ZERO);

    // カーネルとの対話はどのランタイムでも同じ。
    click(page, emu, "lock");
    page.until("ロックした", |s| {
        s["state"]["status"]["locked"] == true
    });
    click(page, emu, "lock");
    page.until("ロックを外した", |s| {
        s["state"]["status"]["locked"] == false
    });

    // 右クリック = グリップ: パネルを掴んでマウスで右へ運ぶと、パネルの外だった所に映る
    // （元の場所には下に重なった GPUI の Testing が見えるので、そちらでは確かめない）。
    let grip = on_screen(emu, &status, (20.0, h - 20.0));
    emu.point(Some(grip));
    emu.button(Button::Grab, true);
    page.until("掴んでいる", |s| {
        s["state"]["status"]["grabbed"] == true
    });
    for i in 1..=10 {
        emu.point(Some(grip + Vec2::new(20.0 * i as f32, 0.0)));
    }
    emu.until_color("パネルが動いた", outside, near([0x20, 0x20, 0x20]));
    emu.button(Button::Grab, false);
    page.until("離した", |s| s["state"]["status"]["grabbed"] == false);

    // マウスが画面の外へ出ると右手が消え、leave が届く。
    emu.point(None);
    page.until("離れた", |s| s["state"]["hovered"] == false);
}
