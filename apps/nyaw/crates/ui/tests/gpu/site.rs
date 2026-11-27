//! ウェブサイト用のキャプチャ: チュートリアルと同じ手順で画面を進め、各オーバーレイの姿勢・大きさ・画素をテーマと言語ごとに
//! `apps/web/public/scene/<テーマ>-<言語>/<場面>/` へ PNG と `scene.json` で書き出す（ウェブサイトが three.js で組み立てる）。
//! `cargo test -p ui --test gpu -- --ignored site` で実行する。

use crate::common;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::Driven;
use dashboard::CoachSpot;
use kernel::vr::{Device, Frame, OverlayId};
use kernel::{
    Config, Kernel, Language, LodQuality, Pose, Runtime, Setting, ThemeMode, VrBackend, VrError,
};

const SCREEN: (u32, u32) = (1280, 720);
/// 書き出す画素の倍率。論理サイズは変えずに画素だけを増やす。
const DENSITY: f32 = 2.0;
/// 書き出す場面と、そこまでに押す「次へ」の回数（チュートリアルの手順の番号）と、その場面でのコーチの位置。
const MOMENTS: [(&str, usize); 2] = [("quick-settings", 4), ("mirroring", 10)];
const COACH_SPOTS: [CoachSpot; 10] = [
    CoachSpot::Docked,
    CoachSpot::Bar,
    CoachSpot::Bar,
    CoachSpot::Bar,
    CoachSpot::Bar,
    CoachSpot::Beside,
    CoachSpot::Beside,
    CoachSpot::Below,
    CoachSpot::Bar,
    CoachSpot::Beside,
];
const LOOKS: [(&str, ThemeMode, Language); 4] = [
    ("dark-en", ThemeMode::Dark, Language::En),
    ("dark-ja", ThemeMode::Dark, Language::Ja),
    ("light-ja", ThemeMode::Light, Language::Ja),
    ("light-en", ThemeMode::Light, Language::En),
];

#[derive(Clone, Default)]
struct Quad {
    key: String,
    width_m: f32,
    height_m: Option<f32>,
    pose: Pose,
    visible: bool,
    alpha: f32,
    sort: u32,
    texture: Option<(u32, u32, Vec<u8>)>,
}

type Quads = Arc<Mutex<BTreeMap<u64, Quad>>>;

/// エミュレーターへの操作をそのまま渡しつつ、オーバーレイの最新の状態を記録する。
struct Recorder {
    inner: Driven,
    quads: Quads,
}

impl Recorder {
    fn with(&self, id: OverlayId, f: impl FnOnce(&mut Quad)) {
        f(self.quads.lock().unwrap().entry(id.0).or_default());
    }
}

impl VrBackend for Recorder {
    fn runtime(&self) -> Runtime {
        self.inner.runtime()
    }
    fn poll(&mut self) -> Frame {
        self.inner.poll()
    }
    fn compositor_time(&self) -> Duration {
        self.inner.compositor_time()
    }
    fn frame_interval(&self) -> Option<Duration> {
        self.inner.frame_interval()
    }
    fn overlay_seen(&self, id: OverlayId) -> Option<bool> {
        self.inner.overlay_seen(id)
    }
    fn create_overlay(&mut self, key: &str, width_m: f32) -> Result<OverlayId, VrError> {
        let id = self.inner.create_overlay(key, width_m)?;
        self.with(id, |q| {
            *q = Quad {
                key: key.into(),
                width_m,
                visible: true,
                alpha: 1.0,
                ..Quad::default()
            }
        });
        Ok(id)
    }
    fn set_overlay_pose(&mut self, id: OverlayId, pose: Pose) {
        self.with(id, |q| q.pose = pose);
        self.inner.set_overlay_pose(id, pose)
    }
    fn set_overlay_pose_relative(&mut self, _: OverlayId, _: &Device, _: Pose) -> bool {
        false
    }
    fn set_overlay_width(&mut self, id: OverlayId, width_m: f32) {
        self.with(id, |q| q.width_m = width_m);
        self.inner.set_overlay_width(id, width_m)
    }
    fn set_overlay_height(&mut self, id: OverlayId, height_m: f32) {
        self.with(id, |q| q.height_m = Some(height_m));
        self.inner.set_overlay_height(id, height_m)
    }
    fn set_overlay_visible(&mut self, id: OverlayId, visible: bool) {
        self.with(id, |q| q.visible = visible);
        self.inner.set_overlay_visible(id, visible)
    }
    fn set_overlay_alpha(&mut self, id: OverlayId, alpha: f32) -> bool {
        self.with(id, |q| q.alpha = alpha);
        self.inner.set_overlay_alpha(id, alpha)
    }
    fn set_overlay_sort(&mut self, id: OverlayId, order: u32) {
        self.with(id, |q| q.sort = order);
        self.inner.set_overlay_sort(id, order)
    }
    fn destroy_overlay(&mut self, id: OverlayId) {
        self.with(id, |q| q.visible = false);
        self.inner.destroy_overlay(id)
    }
    fn submit_overlay_pixels(&mut self, id: OverlayId, width: u32, height: u32, rgba: &[u8]) {
        self.with(id, |q| q.texture = Some((width, height, rgba.to_vec())));
        self.inner.submit_overlay_pixels(id, width, height, rgba)
    }
}

pub fn main() -> ExitCode {
    if !common::should_run("site") {
        return ExitCode::SUCCESS;
    }
    let (inner, emu) = Driven::headless(SCREEN.0, SCREEN.1);
    let quads = Quads::default();
    let backend = Recorder {
        inner,
        quads: quads.clone(),
    };
    // 遠いパネルの解像度を下げない。
    let mut config = Config::default();
    config.kernel.lod_quality = LodQuality::Off;
    let kernel = Kernel::with_backend(Box::new(backend) as Box<dyn VrBackend>, config);
    let mut panels = vec![wrist::spec(
        &kernel.settings(),
        |from, opened| testing::spec(from, opened),
        |from, opened| settings::spec(from, opened),
    )];
    panels.extend(beam::specs());
    panels.push(setup::spec());
    let (remote, rx) = channel::<ui::Remote>();
    let result: Arc<Mutex<Option<bool>>> = Arc::default();
    let done = result.clone();
    std::thread::spawn(move || {
        let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            scenario(&emu, &remote, &quads)
        }))
        .is_ok();
        println!("site: {}", if ok { "ok" } else { "FAILED" });
        *done.lock().unwrap() = Some(ok);
        common::quit(&remote, ok);
    });
    let hooks = ui::Hooks {
        remote: Some(rx),
        hide_profiler: true,
    };
    ui::run_with(kernel, panels, None, hooks);
    common::exit_code("site", &result)
}

fn scenario(emu: &common::Emu, remote: &Sender<ui::Remote>, quads: &Quads) {
    crate::tutorial::pass_setup(emu);
    crate::tutorial::open_dashboard(emu);
    let mut step = 0;
    for (moment, at) in MOMENTS {
        while step < at {
            crate::tutorial::click(emu, crate::tutorial::forward(emu, COACH_SPOTS[step]));
            step += 1;
        }
        emu.point(None);
        if moment == "quick-settings" {
            press_clock(emu, quads);
        }
        // 次の見た目と必ず違う状態にしてから切り替え、すべてのパネルを描き直させる。
        apply(remote, ThemeMode::Light, Language::En);
        for (look, theme, language) in LOOKS {
            apply(remote, theme, language);
            let recorded = quads.lock().unwrap().clone();
            write(&recorded, &sharp(remote), look, moment);
        }
        if moment == "quick-settings" {
            press_clock(emu, quads);
        }
    }
}

/// ダッシュボードのバーの時計（左から 2 つ目のボタン）を押して、クイック設定を開閉する。
fn press_clock(emu: &common::Emu, quads: &Quads) {
    let (pose, width_m, (w, h, rgba)) = {
        let quads = quads.lock().unwrap();
        let bar = quads
            .values()
            .find(|q| q.visible && role(q, "") == Some("dashboard"))
            .expect("the dashboard is shown");
        (bar.pose, bar.width_m, bar.texture.clone().unwrap())
    };
    // バーは横幅の中央に寄せて描かれる。描かれた範囲の左端から、時計の中心までの距離（px）。
    const CLOCK_FROM_LEFT: u32 = 143;
    let left = (0..w)
        .find(|&x| (0..h).any(|y| rgba[((y * w + x) * 4 + 3) as usize] > 0))
        .expect("the bar is drawn");
    let at = ((left + CLOCK_FROM_LEFT) as f32, h as f32 / 2.0);
    let point = crate::tutorial::on_panel(emu, pose, (w as f32, h as f32), width_m, at);
    crate::tutorial::click(emu, point);
    emu.point(None);
}

fn apply(remote: &Sender<ui::Remote>, theme: ThemeMode, language: Language) {
    let _ = remote.send(Box::new(move |cx| {
        ui::set(Setting::Theme(theme), cx);
        ui::set(Setting::Language(language), cx);
    }));
    // テーマの切り替えのフェードと、描画の読み戻しを待つ。
    std::thread::sleep(Duration::from_millis(1500));
}

/// すべてのパネルのウィンドウの描画の倍率を変える。論理サイズは変えない。
fn rescale(remote: &Sender<ui::Remote>, scale: f32) {
    let _ = remote.send(Box::new(move |cx| {
        use gpui::AppContext as _;
        for handle in cx.windows() {
            let _ = cx.update_window(handle, |_, window, _| {
                let size = window.viewport_size();
                #[cfg(windows)]
                gpui_windows::with_render_scale(scale, || window.resize(size));
                #[cfg(not(windows))]
                let _ = (scale, size);
            });
        }
    }));
    std::thread::sleep(Duration::from_millis(500));
}

/// すべてのパネルのウィンドウを [`DENSITY`] 倍の画素で描いて読み戻す。フレームループは寸法の違う絵を捨てるので、
/// ここで直接読み戻し、終えたら等倍に戻す。
fn sharp(remote: &Sender<ui::Remote>) -> Vec<ui::Frame> {
    rescale(remote, DENSITY);
    let (tx, rx) = channel();
    let _ = remote.send(Box::new(move |cx| {
        use gpui::AppContext as _;
        let frames = cx.windows().into_iter().filter_map(|handle| {
            cx.update_window(handle, |_, window, cx| {
                // 読み戻しは 1 回遅れのため、2 回描いて今の絵を受け取る。
                let _ = ui::capture(window, cx);
                ui::capture(window, cx).ok()
            })
            .ok()
            .flatten()
        });
        let _ = tx.send(frames.collect::<Vec<_>>());
    }));
    let frames = rx.recv_timeout(common::TIMEOUT).expect("no frames");
    rescale(remote, 1.0);
    frames
}

/// ウェブサイトでの呼び名。チュートリアルの案内と暗転の幕は書き出さない。
fn role(quad: &Quad, moment: &str) -> Option<&'static str> {
    let (w, h, _) = quad.texture.as_ref()?;
    if quad.key == "nyaw.veil" || (*w, *h) == dashboard::COACH_PX {
        return None;
    }
    Some(if quad.width_m < 0.2 {
        "wrist"
    } else if *w > *h * 5 {
        "dashboard"
    } else if moment == "mirroring" {
        "mirroring"
    } else {
        "quick-settings"
    })
}

/// `frames` のうち、等倍の画素 `texture` と同じ絵を [`DENSITY`] 倍で描いたもの。
fn sharp_of<'a>(
    frames: &'a [ui::Frame],
    (w, h, rgba): &(u32, u32, Vec<u8>),
) -> Option<&'a ui::Frame> {
    let k = DENSITY as u32;
    let sized = frames
        .iter()
        .filter(|f| (f.width, f.height) == (w * k, h * k));
    sized.min_by_key(|f| {
        let mut diff = 0u64;
        for y in (0..*h).step_by(4) {
            for x in (0..*w).step_by(4) {
                let i = ((y * w + x) * 4) as usize;
                let j = ((y * k * f.width + x * k) * 4) as usize;
                for c in 0..4 {
                    diff += u64::from(rgba[i + c].abs_diff(f.rgba[j + c]));
                }
            }
        }
        diff
    })
}

/// 見えているパネルを `<look>/<moment>/` に書き出す。
fn write(quads: &BTreeMap<u64, Quad>, frames: &[ui::Frame], look: &str, moment: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../web/public/scene")
        .join(look)
        .join(moment);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut json = String::from("[\n");
    let mut roles = Vec::new();
    for quad in quads.values().filter(|q| q.visible && q.alpha > 0.0) {
        let (Some(role), Some(texture)) = (role(quad, moment), &quad.texture) else {
            continue;
        };
        assert!(
            !roles.contains(&role),
            "{look}/{moment}: two panels are {role}"
        );
        roles.push(role);
        let frame = sharp_of(frames, texture).unwrap_or_else(|| panic!("{role}: no window"));
        let out = std::io::BufWriter::new(
            std::fs::File::create(dir.join(format!("{role}.png"))).unwrap(),
        );
        let mut png = png::Encoder::new(out, frame.width, frame.height);
        png.set_color(png::ColorType::Rgba);
        png.set_compression(png::Compression::High);
        png.write_header()
            .unwrap()
            .write_image_data(&frame.rgba)
            .unwrap();
        let (p, o) = (quad.pose.position, quad.pose.orientation);
        let height = quad
            .height_m
            .unwrap_or(quad.width_m * texture.1 as f32 / texture.0 as f32);
        let _ = writeln!(
            json,
            "  {{\"role\": \"{role}\", \"width\": {}, \"height\": {height}, \"position\": [{}, {}, {}], \
             \"orientation\": [{}, {}, {}, {}], \"sort\": {}}},",
            quad.width_m, p.x, p.y, p.z, o.x, o.y, o.z, o.w, quad.sort
        );
    }
    let json = json.trim_end_matches(",\n").to_string() + "\n]\n";
    std::fs::write(dir.join("scene.json"), json).unwrap();
    println!("site: {look}/{moment} written ({})", roles.join(", "));
}
