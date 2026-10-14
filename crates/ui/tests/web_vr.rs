//! VR（`Surface::Overlay`）の E2E を実プラットフォームの WebView で流す。
//! 仮想コントローラのレーザー → オーバーレイ層 → カーネル → Tauri のイベント → ブリッジ → DOM のポインタイベント → ページの JS、
//! ページの描画 → 非表示の WebView の読み戻し → VR パネル、ページ → Tauri のコマンド → カーネル、リサイズを確かめる。
//! Linux は画面（Xvfb でよい）があれば `cargo test -p webui --test webview_vr` で流れる。

mod common;

use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{Page, SharedMock, num};
use kernel::vr::mock::MockBackend;
use kernel::vr::{Quat, RawController, Vec3};
use kernel::{Hand, Kernel, Pose, Vec2};
use tauri::Manager;
use webui::WebPanelSpec;

const PANEL_POS: Vec3 = Vec3::new(0.0, 0.0, -1.0);
/// 1m あたりのピクセル数（800px = 0.8m）。リサイズしても変わらない。
const PX_PER_M: f32 = 1000.0;
const PROFILE: &str = "valve/index_controller";

fn main() -> ExitCode {
    if !common::should_run("webview_vr") {
        return ExitCode::SUCCESS;
    }
    let mock = SharedMock::default();
    // 手ブレ抑えは指した瞬間に当たらなくなるので切る（testkit と同じ）。
    let config = kernel::config::Config {
        pointer_smoothing: Some(0.0),
        ..Default::default()
    };
    let kernel = Kernel::with_backend(mock.clone(), config);
    let spec = WebPanelSpec::new(Pose::new(PANEL_POS, Quat::IDENTITY), webtesting::page())
        .sized(Vec2::new(0.8, 0.6), (800, 600));
    common::run("webview_vr", kernel, vec![spec], move |page| {
        let mut vr = Vr {
            mock: mock.0,
            aim: Pose::new(Vec3::new(0.2, -0.3, 0.0), Quat::IDENTITY),
            inputs: Vec::new(),
            panel: PANEL_POS,
            resolution: (800, 600),
        };
        scenario(page, &mut vr);
    })
}

/// 右手の仮想コントローラ。値はモックの `frame` に書き、フレームループが次の tick で読む。
struct Vr {
    mock: Arc<Mutex<MockBackend>>,
    aim: Pose,
    inputs: Vec<(String, f32)>,
    /// 操作するパネルの中心。
    panel: Vec3,
    /// 今のパネルの解像度（ピクセル → 空間の点の変換に使う）。
    resolution: (u32, u32),
}

impl Vr {
    fn send(&self) {
        self.mock.lock().unwrap().frame.controllers = vec![RawController {
            hand: Hand::Right,
            profile: PROFILE.into(),
            aim: self.aim,
            inputs: self.inputs.clone(),
        }];
    }

    /// レーザーをパネル上のピクセル（CSS px）へ向ける。
    fn point_at(&mut self, (x, y): (f32, f32)) {
        let (w, h) = (self.resolution.0 as f32, self.resolution.1 as f32);
        let target =
            self.panel + Vec3::new((x - w / 2.0) / PX_PER_M, (h / 2.0 - y) / PX_PER_M, 0.0);
        let dir = (target - self.aim.position).normalize();
        self.aim.orientation = Quat::from_rotation_arc(Vec3::NEG_Z, dir);
        self.send();
    }

    fn set(&mut self, input: &str, value: f32) {
        self.inputs.retain(|(n, _)| n != input);
        if value != 0.0 {
            self.inputs.push((input.into(), value));
        }
        self.send();
    }

    /// VR パネル（オーバーレイ 0）に最後に送られたテクスチャの、論理ピクセル `(x, y)` の色。
    fn color_at(&self, (x, y): (f32, f32)) -> Option<[u8; 4]> {
        let mock = self.mock.lock().unwrap();
        let tex = mock.overlays[0].texture.as_ref()?;
        let (rw, rh) = self.resolution;
        let tx = ((x / rw as f32 * tex.width as f32) as u32).min(tex.width - 1);
        let ty = ((y / rh as f32 * tex.height as f32) as u32).min(tex.height - 1);
        Some(tex.pixel(tx, ty))
    }

    fn texture_size(&self) -> Option<(u32, u32)> {
        self.texture_size_of(0)
    }

    fn texture_size_of(&self, overlay: usize) -> Option<(u32, u32)> {
        let mock = self.mock.lock().unwrap();
        mock.overlays[overlay]
            .texture
            .as_ref()
            .map(|t| (t.width, t.height))
    }

    /// VR パネルの `(x, y)` が `rgb` に近い色になるまで待つ。
    fn until_color(&self, what: &str, at: (f32, f32), rgb: [u8; 3]) {
        let deadline = Instant::now() + common::TIMEOUT;
        loop {
            let c = self.color_at(at);
            if c.is_some_and(|[r, g, b, _]| {
                [r, g, b]
                    .iter()
                    .zip(rgb)
                    .all(|(a, b)| (*a as i32 - b as i32).abs() <= 8)
            }) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{what}: VR パネルの {at:?} が {rgb:02x?} にならない（{c:02x?}）"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn scenario(page: &mut Page, vr: &mut Vr) {
    // 非表示の WebView が読み戻されて VR に届く（背景は #202020）。
    vr.until_color("ページの描画が VR に届く", (4.0, 590.0), [0x20, 0x20, 0x20]);
    assert_eq!(
        vr.texture_size(),
        Some((800, 600)),
        "論理ピクセル × 拡大率 1"
    );

    // ホバー: pointerenter が JS に届き、CSS 用の data-nyaw-hover が付く。
    let click_me = page.center("click-me");
    vr.point_at(click_me);
    let s = page.until("レーザーが乗る", |s| s["state"]["hovered"] == true);
    assert_eq!(s["hoverAttr"], true);
    assert!(num(&s, &["state", "counts", "pointerover"]) >= 1.0);
    assert!(num(&s, &["state", "counts", "mouseenter"]) >= 1.0);

    // クリック: 押して離すと pointerdown → mousedown → pointerup → mouseup → click が 1 回ずつ。
    vr.set("trigger/value", 1.0);
    let s = page.until("押した", |s| {
        num(s, &["state", "counts", "pointerdown"]) == 1.0
    });
    let down = &s["state"]["lastPointer"];
    assert_eq!(
        (&down["type"], &down["target"], &down["pointerType"]),
        (&"pointerdown".into(), &"click-me".into(), &"mouse".into())
    );
    assert_eq!(down["buttons"], 1);
    assert_eq!(down["trusted"], false, "ブリッジが合成したイベント");
    vr.set("trigger/value", 0.0);
    let s = page.until("クリックした", |s| s["state"]["clicks"] == 1);
    for t in ["pointerdown", "mousedown", "pointerup", "mouseup", "click"] {
        assert_eq!(num(&s, &["state", "counts", t]), 1.0, "{t}");
    }
    // クリックで緑になったボタンが VR にも映る。
    let edge = page.left_edge("click-me", 3.0);
    vr.until_color("クリックの結果が VR に映る", edge, [0x2e, 0xcc, 0x71]);

    // ドラッグ: setPointerCapture したチップを、ドロップ先まで運んで離す。
    let chip = page.center("drag-chip");
    let zone = page.center("drop-zone");
    vr.point_at(chip);
    page.until("チップに乗る", |s| {
        s["state"]["last"]["target"] == "drag-chip"
    });
    vr.set("trigger/value", 1.0);
    page.until("キャプチャした", |s| s["state"]["captured"] == true);
    for i in 1..=10 {
        let t = i as f32 / 10.0;
        vr.point_at((
            chip.0 + (zone.0 - chip.0) * t,
            chip.1 + (zone.1 - chip.1) * t,
        ));
        std::thread::sleep(Duration::from_millis(15));
    }
    page.until("運んでいる", |s| num(s, &["state", "dragMoves"]) > 0.0);
    vr.set("trigger/value", 0.0);
    let s = page.until("落とした", |s| s["state"]["dropped"] == true);
    assert_eq!(s["state"]["captured"], false, "離すとキャプチャも外れる");

    // スクロール: スティックを下へ倒すと wheel（deltaY > 0）が届き、一覧が下へ動く。
    vr.point_at(page.center("scroll-list"));
    vr.set("thumbstick/y", -1.0);
    page.until("スクロールした", |s| {
        num(s, &["state", "wheel"]) > 0.0 && num(s, &["state", "scrollTop"]) > 0.0
    });
    vr.set("thumbstick/y", 0.0);

    // フォーカス: 入力欄を押すとフォーカスが移る。
    click(page, vr, "text-field");
    page.until("入力欄にフォーカス", |s| {
        s["focused"] == "text-field"
    });

    // カーネルとの対話: ボタン → Tauri のコマンド → カーネル → 状態のイベント → ページ。
    click(page, vr, "lock");
    page.until("ロックした", |s| {
        s["state"]["status"]["locked"] == true
    });
    click(page, vr, "attach-hmd"); // HMD は原点なので、取り付けても同じ所（1m 前）にある
    page.until("HMD に取り付けた", |s| {
        s["state"]["status"]["attached"] == "hmd"
    });
    click(page, vr, "detach");
    page.until("取り外した", |s| {
        s["state"]["status"]["attached"].is_null()
    });
    click(page, vr, "lock");
    page.until("ロックを外した", |s| {
        s["state"]["status"]["locked"] == false
    });

    // パネルを掴む: オーバーレイ層の操作も同じく効き、状態がページに届く。
    vr.point_at(page.center("log"));
    vr.set("squeeze/value", 1.0);
    page.until("掴んでいる", |s| {
        s["state"]["status"]["grabbed"] == true
    });
    vr.set("squeeze/value", 0.0);
    page.until("離した", |s| s["state"]["status"]["grabbed"] == false);

    // リサイズ: ビューポート・カーネルの解像度・VR 上の幅・送られるテクスチャがそろって変わる。
    click(page, vr, "resize-small");
    let s = page.until("小さくなった", |s| {
        s["state"]["viewport"] == serde_json::json!([640, 480])
            && s["state"]["status"]["resolution"] == serde_json::json!([640, 480])
    });
    assert_eq!(num(&s, &["state", "resizes"]), 1.0, "JS に resize イベント");
    vr.resolution = (640, 480);
    assert!((vr.mock.lock().unwrap().overlays[0].width_m - 0.64).abs() < 1e-4);
    let deadline = Instant::now() + common::TIMEOUT;
    while vr.texture_size() != Some((640, 480)) {
        assert!(Instant::now() < deadline, "VR に新しい大きさで届かない");
        std::thread::sleep(Duration::from_millis(20));
    }
    // 新しいレイアウトでもレーザーの当たる位置が合っている。
    click(page, vr, "click-me");
    page.until("リサイズ後もクリックできる", |s| {
        s["state"]["clicks"] == 2
    });

    // 隣にパネルを開き（spawn_panel コマンド）、そのパネルの Close で閉じる（カーネルとウィンドウの両方）。
    click(page, vr, "spawn-panel");
    let downs = num(&page.probe(), &["state", "counts", "pointerdown"]);
    let deadline = Instant::now() + common::TIMEOUT;
    while vr.mock.lock().unwrap().overlays.len() < 2 {
        assert!(Instant::now() < deadline, "パネルが開かない");
        std::thread::sleep(Duration::from_millis(20));
    }
    page.label = "panel-1".into();
    page.until("開いたパネルが読み込まれる", |s| {
        s["state"]["status"]["panel"] == 1
    });
    (vr.panel, vr.resolution) = (PANEL_POS + Vec3::new(1.2, 0.0, 0.0), (800, 600));
    press(page, vr, "close");
    let deadline = Instant::now() + common::TIMEOUT;
    while page.app.get_webview_window("panel-1").is_some() {
        assert!(Instant::now() < deadline, "ウィンドウが閉じない");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        vr.texture_size_of(1),
        Some((1, 1)),
        "閉じたパネルは透明な 1px"
    );
    page.label = "panel-0".into();
    (vr.panel, vr.resolution) = (PANEL_POS, (640, 480));
    let s = page.probe();
    assert_eq!(
        num(&s, &["state", "counts", "pointerdown"]),
        downs,
        "隣のパネルへのレーザーはこのパネルに届かない"
    );

    // パネルの外へ出ると leave。
    vr.aim.orientation = Quat::IDENTITY;
    vr.aim.position = Vec3::new(5.0, 0.0, 0.0);
    vr.send();
    page.until("離れた", |s| {
        s["state"]["hovered"] == false && s["hoverAttr"] == false
    });
}

/// 要素の中心を指して、選択ボタンを押して離し、click が届くのを待つ。
fn click(page: &mut Page, vr: &mut Vr, id: &str) {
    press(page, vr, id);
    page.until(&format!("{id} を離した"), |s| {
        s["state"]["last"]["type"] == "click" || s["state"]["last"]["type"] == "lostpointercapture"
    });
}

/// 要素の中心を指して、選択ボタンを押して離す（離した後のページは待たない）。
fn press(page: &mut Page, vr: &mut Vr, id: &str) {
    vr.point_at(page.center(id));
    page.until(&format!("{id} に乗る"), |s| {
        s["state"]["last"]["target"] == id
    });
    let downs = num(&page.probe(), &["state", "counts", "pointerdown"]);
    vr.set("trigger/value", 1.0);
    page.until(&format!("{id} を押した"), |s| {
        num(s, &["state", "counts", "pointerdown"]) > downs
    });
    vr.set("trigger/value", 0.0);
}
