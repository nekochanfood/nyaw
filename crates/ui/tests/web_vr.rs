//! VR（モックのランタイム）の Web のパネルの E2E。本番と同じ `ui::run_with`（GPUI のイベントループの上で Tauri も回す）で、
//! 実プラットフォームの WebView に Testing の Web 版（React + Base UI）を載せて流す。
//! 仮想コントローラのレーザー → オーバーレイ層 → カーネル → Tauri のイベント → ブリッジ → DOM のポインタイベント → React / Base UI、
//! ページの描画 → 画面に出さない WebView の読み戻し → VR パネル（半透明を含む）、ページ → Tauri のコマンド → カーネル、
//! リサイズを確かめる。Linux は画面（Xvfb でよい）があれば `cargo test -p ui --test web_vr` で流れる。

mod common;

use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{Page, SharedMock, num};
use kernel::vr::mock::MockBackend;
use kernel::vr::{Quat, RawController, Vec3};
use kernel::{Hand, Kernel, Pose, Vec2, VrBackend};
use serde_json::{Value, json};
use tauri::Manager;
use ui::PanelSpec;

const PANEL_POS: Vec3 = Vec3::new(0.0, 0.0, -1.0);
/// 1m あたりのピクセル数（800px = 0.8m）。リサイズしても変わらない。
const PX_PER_M: f32 = 1000.0;
const PROFILE: &str = "valve/index_controller";

fn main() -> ExitCode {
    if !common::should_run("web_vr") {
        return ExitCode::SUCCESS;
    }
    let mock = SharedMock::default();
    // 手ブレ抑えは指した瞬間に当たらなくなるので切る（testkit と同じ）。
    let config = kernel::config::Config {
        pointer_smoothing: Some(0.0),
        // 操作のシナリオはマウスで書く（既定のタッチは web_pointer で確かめる）。
        pointer_mode: Some(kernel::PointerMode::Mouse),
        ..Default::default()
    };
    let kernel = Kernel::with_backend(Box::new(mock.clone()) as Box<dyn VrBackend>, config);
    let spec = PanelSpec::web(Pose::new(PANEL_POS, Quat::IDENTITY), webpages::TESTING)
        .sized(Vec2::new(0.8, 0.6), (800, 600));
    common::run(
        "web_vr",
        kernel,
        vec![spec],
        "panel-0",
        None,
        move |page, _| {
            let mut vr = Vr {
                mock: mock.0,
                aim: Pose::new(Vec3::new(0.2, -0.3, 0.0), Quat::IDENTITY),
                inputs: Vec::new(),
                panel: PANEL_POS,
                resolution: (800, 600),
                overlay: 0,
            };
            scenario(page, &mut vr);
        },
    )
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
    /// 操作するパネルのオーバーレイ（= PanelId）。
    overlay: usize,
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

    /// VR パネルに最後に送られたテクスチャの、論理ピクセル `(x, y)` の色（RGBA、ストレートアルファ）。
    fn color_at(&self, (x, y): (f32, f32)) -> Option<[u8; 4]> {
        let mock = self.mock.lock().unwrap();
        let tex = mock.overlays[self.overlay].texture.as_ref()?;
        let (rw, rh) = self.resolution;
        let tx = ((x / rw as f32 * tex.width as f32) as u32).min(tex.width - 1);
        let ty = ((y / rh as f32 * tex.height as f32) as u32).min(tex.height - 1);
        Some(tex.pixel(tx, ty))
    }

    fn texture_size(&self, overlay: usize) -> Option<(u32, u32)> {
        let mock = self.mock.lock().unwrap();
        mock.overlays
            .get(overlay)?
            .texture
            .as_ref()
            .map(|t| (t.width, t.height))
    }

    /// VR パネルの `(x, y)` が `rgba` に近い色（各成分 ±`tol`）になるまで待つ。`rgb` が None ならアルファだけ見る。
    fn until_rgba(&self, what: &str, at: (f32, f32), rgb: Option<[u8; 3]>, alpha: u8, tol: i32) {
        let deadline = Instant::now() + common::TIMEOUT;
        loop {
            let c = self.color_at(at);
            let near = |a: u8, b: u8| (a as i32 - b as i32).abs() <= tol;
            if c.is_some_and(|[r, g, b, a]| {
                near(a, alpha)
                    && rgb.is_none_or(|[er, eg, eb]| near(r, er) && near(g, eg) && near(b, eb))
            }) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{what}: VR パネルの {at:?} が rgb {rgb:02x?} / alpha {alpha} にならない（{c:02x?}）"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn until_texture(&self, what: &str, overlay: usize, size: (u32, u32)) {
        let deadline = Instant::now() + common::TIMEOUT;
        while self.texture_size(overlay) != Some(size) {
            assert!(
                Instant::now() < deadline,
                "{what}: {:?} のまま",
                self.texture_size(overlay)
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn has(list: &Value, id: &str) -> bool {
    list.as_array().is_some_and(|a| a.iter().any(|v| v == id))
}

fn scenario(page: &mut Page, vr: &mut Vr) {
    // 画面に出さない WebView が読み戻されて VR に届く（背景は不透明な #202020）。
    vr.until_rgba(
        "ページの描画が VR に届く",
        (4.0, 590.0),
        Some([0x20, 0x20, 0x20]),
        255,
        8,
    );
    assert_eq!(
        vr.texture_size(0),
        Some((800, 600)),
        "論理ピクセル × 拡大率 1"
    );

    // ホバー: pointerenter が React に届き、CSS 用の data-nyaw-hover が付く。
    let click_me = page.center("click-me");
    vr.point_at(click_me);
    let s = page.until("レーザーが乗る", |s| s["state"]["hovered"] == true);
    assert!(has(&s["hover"], "click-me"), "{}", s["hover"]);
    // Tailwind の hover: は [data-nyaw-hover] でも効く（ボタンの色が変わって VR に映る）。
    let edge = page.left_edge("click-me", 3.0);
    vr.until_rgba(
        "ホバーの色が VR に映る",
        edge,
        Some([0x50, 0x54, 0x5f]),
        255,
        8,
    );

    // クリック: 押して離すと pointerdown → mousedown → pointerup → mouseup → click が 1 回ずつ。
    vr.set("trigger/value", 1.0);
    let s = page.until("押した", |s| {
        num(s, &["state", "counts", "pointerdown"]) == 1.0
    });
    let down = &s["state"]["lastPointer"];
    assert_eq!(down["type"], "pointerdown");
    assert_eq!(down["target"], "click-me");
    assert_eq!(down["pointerType"], "mouse");
    assert_eq!(down["buttons"], 1);
    assert_eq!(down["trusted"], false, "ブリッジが合成したイベント");
    assert!(
        has(&s["active"], "click-me"),
        "押している間は data-nyaw-active"
    );
    vr.set("trigger/value", 0.0);
    let s = page.until("クリックした", |s| s["state"]["clicks"] == 1);
    for t in ["pointerdown", "mousedown", "pointerup", "mouseup", "click"] {
        assert_eq!(num(&s, &["state", "counts", t]), 1.0, "{t}");
    }
    vr.until_rgba(
        "クリックの結果が VR に映る",
        edge,
        Some([0x2e, 0xcc, 0x71]),
        255,
        8,
    );

    // 押している間の見た目（hold-me）。
    vr.point_at(page.center("hold-me"));
    page.until("hold-me に乗る", |s| has(&s["hover"], "hold-me"));
    vr.set("trigger/value", 1.0);
    let s = page.until("押している", |s| s["state"]["held"] == true);
    assert!(has(&s["active"], "hold-me"));
    vr.set("trigger/value", 0.0);
    page.until("離した", |s| s["state"]["held"] == false);

    // Base UI のコンポーネント: Toggle / Switch / Checkbox / Tabs。
    click(page, vr, "toggle");
    page.until("Toggle", |s| s["state"]["toggled"] == true);
    click(page, vr, "switch");
    page.until("Switch", |s| s["state"]["switched"] == true);
    click(page, vr, "checkbox");
    page.until("Checkbox", |s| s["state"]["checked"] == true);
    click(page, vr, "tab-b");
    page.until("Tabs", |s| s["state"]["tab"] == "tab-b");

    // Slider: つまみを掴んで右端まで動かす（Base UI は setPointerCapture で追う）。
    let thumb = page.center("slider-thumb");
    let s = page.probe();
    let r = &s["rects"]["slider"];
    let right = (r["x"].as_f64().unwrap() + r["w"].as_f64().unwrap()) as f32 + 20.0;
    vr.point_at(thumb);
    page.until("つまみに乗る", |s| has(&s["hover"], "slider-thumb"));
    vr.set("trigger/value", 1.0);
    page.until("つまみを押した", |s| {
        has(&s["active"], "slider-thumb")
    });
    for i in 1..=10 {
        let x = thumb.0 + (right - thumb.0) * i as f32 / 10.0;
        vr.point_at((x, thumb.1));
        std::thread::sleep(Duration::from_millis(15));
    }
    page.until("右端まで動いた", |s| {
        num(s, &["state", "slider"]) == 100.0
    });
    vr.set("trigger/value", 0.0);

    // Menu: 開いて項目を選ぶと閉じる（ポップアップは Portal で body に出る）。
    click(page, vr, "open-menu");
    page.until("メニューが開いた", |s| {
        s["state"]["menuOpen"] == true && s["rects"]["menu-b"].is_object()
    });
    vr.point_at(page.center("menu-b"));
    page.until("項目に乗る", |s| has(&s["hover"], "menu-b"));
    vr.set("trigger/value", 1.0);
    std::thread::sleep(Duration::from_millis(50));
    vr.set("trigger/value", 0.0);
    page.until("選んで閉じた", |s| {
        s["state"]["picked"] == "menu-b" && s["state"]["menuOpen"] == false
    });

    // ドラッグ: setPointerCapture したチップを、ドロップ先まで運んで離す。
    let chip = page.center("drag-chip");
    let zone = page.center("drop-zone");
    vr.point_at(chip);
    page.until("チップに乗る", |s| has(&s["hover"], "drag-chip"));
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

    // スクロール（Base UI ScrollArea）: スティックを下へ倒すと wheel（deltaY > 0）が届き、一覧が下へ動く。
    vr.point_at(page.center("scroll-list"));
    vr.set("thumbstick/y", -1.0);
    page.until("スクロールした", |s| {
        num(s, &["state", "wheel"]) > 0.0 && num(s, &["state", "scrollTop"]) > 0.0
    });
    vr.set("thumbstick/y", 0.0);

    // フォーカス: 入力欄を押すとフォーカスが移る（キーボードは novr のみ）。
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
    vr.point_at(page.center("size"));
    vr.set("squeeze/value", 1.0);
    page.until("掴んでいる", |s| {
        s["state"]["status"]["grabbed"] == true
    });
    vr.set("squeeze/value", 0.0);
    page.until("離した", |s| s["state"]["status"]["grabbed"] == false);

    // リサイズ: ビューポート・カーネルの解像度・VR 上の幅・送られるテクスチャがそろって変わる。
    click(page, vr, "resize-small");
    // JS の resize イベントも届く（React の状態に反映されるまで待つ）。
    page.until("小さくなった", |s| {
        s["viewport"] == json!([640, 480])
            && s["state"]["status"]["resolution"] == json!([640, 480])
            && num(s, &["state", "resizes"]) >= 1.0
    });
    vr.resolution = (640, 480);
    assert!((vr.mock.lock().unwrap().overlays[0].width_m - 0.64).abs() < 1e-4);
    vr.until_texture("VR に新しい大きさで届く", 0, (640, 480));
    // 新しいレイアウトでもレーザーの当たる位置が合っている。
    click(page, vr, "click-me");
    page.until("リサイズ後もクリックできる", |s| {
        s["state"]["clicks"] == 2
    });

    transparent_page(page, vr);

    // パネルの外へ出ると leave。
    vr.aim.orientation = Quat::IDENTITY;
    vr.aim.position = Vec3::new(5.0, 0.0, 0.0);
    vr.send();
    page.until("離れた", |s| {
        s["state"]["hovered"] == false && s["hover"].as_array().is_some_and(|a| a.is_empty())
    });
}

/// 半透明のページを隣に開き（ページ → spawn_panel コマンド）、読み戻したテクスチャのアルファを確かめ、
/// 半透明のボタンを押し、Close で閉じる（カーネルとウィンドウの両方）。
fn transparent_page(page: &mut Page, vr: &mut Vr) {
    let downs = num(&page.probe(), &["state", "counts", "pointerdown"]);
    click(page, vr, "open-transparent");
    let deadline = Instant::now() + common::TIMEOUT;
    while vr.mock.lock().unwrap().overlays.len() < 2 {
        assert!(Instant::now() < deadline, "パネルが開かない");
        std::thread::sleep(Duration::from_millis(20));
    }
    page.label = "panel-1".into();
    let s = page.until("半透明のページが読み込まれる", |s| {
        s["state"]["clicks"] == 0
    });
    (vr.panel, vr.resolution, vr.overlay) = (PANEL_POS + Vec3::new(0.0, 0.0, 0.05), (800, 600), 1);
    let at = |id: &str| {
        let r = &s["rects"][id];
        let f = |k: &str| r[k].as_f64().unwrap() as f32;
        (f("x") + f("w") / 2.0, f("y") + f("h") / 2.0)
    };
    // 何も塗っていない所は透明、塗った所はその不透明度（ストレートアルファで色はそのまま）。
    vr.until_rgba("背景は透明", (790.0, 590.0), None, 0, 2);
    vr.until_rgba("不透明", at("opaque"), Some([0x2e, 0xcc, 0x71]), 255, 8);
    vr.until_rgba("50%", at("half"), Some([0, 0, 0]), 128, 8);
    vr.until_rgba("25%", at("faint"), Some([255, 255, 255]), 64, 8);

    // 半透明のボタンもレーザーで押せる。
    click(page, vr, "glass-button");
    page.until("半透明のボタンを押した", |s| {
        s["state"]["clicks"] == 1
    });

    // Close: このパネルだけ閉じる。
    press(page, vr, "close");
    let deadline = Instant::now() + common::TIMEOUT;
    while page.app.get_webview_window("panel-1").is_some() {
        assert!(Instant::now() < deadline, "ウィンドウが閉じない");
        std::thread::sleep(Duration::from_millis(20));
    }
    let deadline = Instant::now() + common::TIMEOUT;
    while vr.texture_size(1) != Some((1, 1)) {
        assert!(
            Instant::now() < deadline,
            "アウト後にパネルが透明な 1px にならない"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    page.label = "panel-0".into();
    (vr.panel, vr.resolution, vr.overlay) = (PANEL_POS, (640, 480), 0);
    assert_eq!(
        num(&page.probe(), &["state", "counts", "pointerdown"]),
        downs + 1.0,
        "隣のパネルへのレーザーはこのパネルに届かない（open-transparent の 1 回だけ）"
    );
}

/// 要素の中心を指して、選択ボタンを押して離し、click が届くのを待つ。
fn click(page: &mut Page, vr: &mut Vr, id: &str) {
    let clicks = num(&page.probe(), &["state", "counts", "click"]);
    press(page, vr, id);
    page.until(&format!("{id} を離した"), |s| {
        !s["state"]["counts"].is_object() || num(s, &["state", "counts", "click"]) > clicks
    });
}

/// 要素の中心を指して、選択ボタンを押して離す（離した後のページは待たない）。
fn press(page: &mut Page, vr: &mut Vr, id: &str) {
    vr.point_at(page.center(id));
    page.until(&format!("{id} に乗る"), |s| has(&s["hover"], id));
    vr.set("trigger/value", 1.0);
    page.until(&format!("{id} を押した"), |s| has(&s["active"], id));
    vr.set("trigger/value", 0.0);
}
