//! E2E for web panels on VR (mock runtime). Using the production `ui::run_with` (Tauri also runs on top of GPUI's event loop),
//! it runs the web version of Testing (React + Base UI) in the real platform's WebView.
//! It checks virtual controller laser -> overlay layer -> kernel -> Tauri event -> bridge -> DOM pointer events -> React / Base UI,
//! page render -> off-screen WebView readback -> VR panel (including translucency), page -> Tauri command -> kernel,
//! and resizing.On Linux, with a display (Xvfb is fine), `cargo test -p ui --test web_vr` runs it.

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
/// pixels per meter (800px = 0.8m). Unchanged by resizing.
const PX_PER_M: f32 = 1000.0;
const PROFILE: &str = "valve/index_controller";

fn main() -> ExitCode {
    if !common::should_run("web_vr") {
        return ExitCode::SUCCESS;
    }
    let mock = SharedMock::default();
    // turn off hand-shake damping, since it makes pointing miss right after aiming (same as testkit).
    let config = kernel::config::Config {
        pointer_smoothing: Some(0.0),
        // write the scenarios for mouse (the default touch is covered by web_pointer).
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

/// the right-hand virtual controller. Values are written to the mock's `frame` and read by the frame loop on the next tick.
struct Vr {
    mock: Arc<Mutex<MockBackend>>,
    aim: Pose,
    inputs: Vec<(String, f32)>,
    /// center of the panel being operated.
    panel: Vec3,
    /// current panel resolution (used to convert pixels to space points).
    resolution: (u32, u32),
    /// overlay of the panel being operated (= PanelId).
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

    /// aim the laser at a pixel (CSS px) on the panel.
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

    /// color (RGBA, straight alpha) at logical pixel `(x, y)` of the texture last sent to the VR panel.
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

    /// wait until `(x, y)` of the VR panel is close to `rgba` (each component within +-`tol`). If `rgb` is None, only alpha is checked.
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
                "{what}: VR panel at {at:?} never became rgb {rgb:02x?} / alpha {alpha} ({c:02x?})"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn until_texture(&self, what: &str, overlay: usize, size: (u32, u32)) {
        let deadline = Instant::now() + common::TIMEOUT;
        while self.texture_size(overlay) != Some(size) {
            assert!(
                Instant::now() < deadline,
                "{what}: still {:?}",
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
    // the off-screen WebView is read back and reaches VR (background is opaque #202020).
    vr.until_rgba(
        "the page's render reaches VR",
        (4.0, 590.0),
        Some([0x20, 0x20, 0x20]),
        255,
        8,
    );
    assert_eq!(
        vr.texture_size(0),
        Some((800, 600)),
        "logical pixels x scale 1"
    );

    // hover: pointerenter reaches React and data-nyaw-hover is set for CSS.
    let click_me = page.center("click-me");
    vr.point_at(click_me);
    let s = page.until("laser hovers", |s| s["state"]["hovered"] == true);
    assert!(has(&s["hover"], "click-me"), "{}", s["hover"]);
    // Tailwind's hover: also works via [data-nyaw-hover] (the button color changes and shows in VR).
    let edge = page.left_edge("click-me", 3.0);
    vr.until_rgba(
        "hover color shows in VR",
        edge,
        Some([0x50, 0x54, 0x5f]),
        255,
        8,
    );

    // click: press and release gives pointerdown -> mousedown -> pointerup -> mouseup -> click once each.
    vr.set("trigger/value", 1.0);
    let s = page.until("pressed", |s| {
        num(s, &["state", "counts", "pointerdown"]) == 1.0
    });
    let down = &s["state"]["lastPointer"];
    assert_eq!(down["type"], "pointerdown");
    assert_eq!(down["target"], "click-me");
    assert_eq!(down["pointerType"], "mouse");
    assert_eq!(down["buttons"], 1);
    assert_eq!(down["trusted"], false, "event synthesized by the bridge");
    assert!(
        has(&s["active"], "click-me"),
        "data-nyaw-active while pressed"
    );
    vr.set("trigger/value", 0.0);
    let s = page.until("clicked", |s| s["state"]["clicks"] == 1);
    for t in ["pointerdown", "mousedown", "pointerup", "mouseup", "click"] {
        assert_eq!(num(&s, &["state", "counts", t]), 1.0, "{t}");
    }
    vr.until_rgba(
        "click result shows in VR",
        edge,
        Some([0x2e, 0xcc, 0x71]),
        255,
        8,
    );

    // look while pressed (hold-me).
    vr.point_at(page.center("hold-me"));
    page.until("hover over hold-me", |s| has(&s["hover"], "hold-me"));
    vr.set("trigger/value", 1.0);
    let s = page.until("pressing", |s| s["state"]["held"] == true);
    assert!(has(&s["active"], "hold-me"));
    vr.set("trigger/value", 0.0);
    page.until("released", |s| s["state"]["held"] == false);

    // Base UI components: Toggle / Switch / Checkbox / Tabs.
    click(page, vr, "toggle");
    page.until("Toggle", |s| s["state"]["toggled"] == true);
    click(page, vr, "switch");
    page.until("Switch", |s| s["state"]["switched"] == true);
    click(page, vr, "checkbox");
    page.until("Checkbox", |s| s["state"]["checked"] == true);
    click(page, vr, "tab-b");
    page.until("Tabs", |s| s["state"]["tab"] == "tab-b");

    // Slider: grab the thumb and move it to the right end (Base UI follows via setPointerCapture).
    let thumb = page.center("slider-thumb");
    let s = page.probe();
    let r = &s["rects"]["slider"];
    let right = (r["x"].as_f64().unwrap() + r["w"].as_f64().unwrap()) as f32 + 20.0;
    vr.point_at(thumb);
    page.until("hover over thumb", |s| has(&s["hover"], "slider-thumb"));
    vr.set("trigger/value", 1.0);
    page.until("pressed thumb", |s| has(&s["active"], "slider-thumb"));
    for i in 1..=10 {
        let x = thumb.0 + (right - thumb.0) * i as f32 / 10.0;
        vr.point_at((x, thumb.1));
        std::thread::sleep(Duration::from_millis(15));
    }
    page.until("moved to the right end", |s| {
        num(s, &["state", "slider"]) == 100.0
    });
    vr.set("trigger/value", 0.0);

    // Menu: opening and choosing an item closes it (the popup is rendered into body via Portal).
    click(page, vr, "open-menu");
    page.until("menu opened", |s| {
        s["state"]["menuOpen"] == true && s["rects"]["menu-b"].is_object()
    });
    vr.point_at(page.center("menu-b"));
    page.until("hover over item", |s| has(&s["hover"], "menu-b"));
    vr.set("trigger/value", 1.0);
    std::thread::sleep(Duration::from_millis(50));
    vr.set("trigger/value", 0.0);
    page.until("chose and closed", |s| {
        s["state"]["picked"] == "menu-b" && s["state"]["menuOpen"] == false
    });

    // drag: carry the chip that called setPointerCapture to the drop target and release.
    let chip = page.center("drag-chip");
    let zone = page.center("drop-zone");
    vr.point_at(chip);
    page.until("hover over chip", |s| has(&s["hover"], "drag-chip"));
    vr.set("trigger/value", 1.0);
    page.until("captured", |s| s["state"]["captured"] == true);
    for i in 1..=10 {
        let t = i as f32 / 10.0;
        vr.point_at((
            chip.0 + (zone.0 - chip.0) * t,
            chip.1 + (zone.1 - chip.1) * t,
        ));
        std::thread::sleep(Duration::from_millis(15));
    }
    page.until("dragging", |s| num(s, &["state", "dragMoves"]) > 0.0);
    vr.set("trigger/value", 0.0);
    let s = page.until("dropped", |s| s["state"]["dropped"] == true);
    assert_eq!(
        s["state"]["captured"], false,
        "releasing also drops the capture"
    );

    // scroll (Base UI ScrollArea): tilting the stick down delivers wheel (deltaY > 0) and the list moves down.
    vr.point_at(page.center("scroll-list"));
    vr.set("thumbstick/y", -1.0);
    page.until("scrolled", |s| {
        num(s, &["state", "wheel"]) > 0.0 && num(s, &["state", "scrollTop"]) > 0.0
    });
    vr.set("thumbstick/y", 0.0);

    // focus: pressing the input moves focus (keyboard is novr only).
    click(page, vr, "text-field");
    page.until("input focused", |s| s["focused"] == "text-field");

    // interaction with the kernel: button -> Tauri command -> kernel -> status event -> page.
    click(page, vr, "lock");
    page.until("locked", |s| s["state"]["status"]["locked"] == true);
    click(page, vr, "attach-hmd"); // the HMD is at the origin, so attaching keeps it at the same place (1m ahead)
    page.until("attached to HMD", |s| {
        s["state"]["status"]["attached"] == "hmd"
    });
    click(page, vr, "detach");
    page.until("detached", |s| s["state"]["status"]["attached"].is_null());
    click(page, vr, "lock");
    page.until("unlocked", |s| s["state"]["status"]["locked"] == false);

    // grab the panel: overlay-layer operations work the same and the status reaches the page.
    vr.point_at(page.center("size"));
    vr.set("squeeze/value", 1.0);
    page.until("grabbing", |s| s["state"]["status"]["grabbed"] == true);
    vr.set("squeeze/value", 0.0);
    page.until("released", |s| s["state"]["status"]["grabbed"] == false);

    // resize: the viewport, kernel resolution, width in VR, and sent texture all change together.
    click(page, vr, "resize-small");
    // the JS resize event also arrives (wait until it is reflected in React state).
    page.until("got smaller", |s| {
        s["viewport"] == json!([640, 480])
            && s["state"]["status"]["resolution"] == json!([640, 480])
            && num(s, &["state", "resizes"]) >= 1.0
    });
    vr.resolution = (640, 480);
    assert!((vr.mock.lock().unwrap().overlays[0].width_m - 0.64).abs() < 1e-4);
    vr.until_texture("arrives in VR at the new size", 0, (640, 480));
    // the laser hit position is still correct in the new layout.
    click(page, vr, "click-me");
    page.until("can click after resize", |s| s["state"]["clicks"] == 2);

    transparent_page(page, vr);

    // leaving the panel gives leave.
    vr.aim.orientation = Quat::IDENTITY;
    vr.aim.position = Vec3::new(5.0, 0.0, 0.0);
    vr.send();
    page.until("left", |s| {
        s["state"]["hovered"] == false && s["hover"].as_array().is_some_and(|a| a.is_empty())
    });
}

/// open a translucent page next to it (page -> spawn_panel command), check the alpha of the read-back texture,
/// press a translucent button, and close with Close (both the kernel and the window).
fn transparent_page(page: &mut Page, vr: &mut Vr) {
    let downs = num(&page.probe(), &["state", "counts", "pointerdown"]);
    click(page, vr, "open-transparent");
    let deadline = Instant::now() + common::TIMEOUT;
    while vr.mock.lock().unwrap().overlays.len() < 2 {
        assert!(Instant::now() < deadline, "panel does not open");
        std::thread::sleep(Duration::from_millis(20));
    }
    // wait until the opened panel's WebView (with its label if assigned from the spare) is visible.
    let label = loop {
        let label = ui::web::label_of(&page.app, 1);
        if page.app.get_webview_window(&label).is_some() {
            break label;
        }
        assert!(Instant::now() < deadline, "no panel window");
        std::thread::sleep(Duration::from_millis(20));
    };
    page.label = label.clone();
    let s = page.until("translucent page loaded", |s| s["state"]["clicks"] == 0);
    (vr.panel, vr.resolution, vr.overlay) = (PANEL_POS + Vec3::new(0.0, 0.0, 0.05), (800, 600), 1);
    let at = |id: &str| {
        let r = &s["rects"][id];
        let f = |k: &str| r[k].as_f64().unwrap() as f32;
        (f("x") + f("w") / 2.0, f("y") + f("h") / 2.0)
    };
    // unpainted areas are transparent, painted areas have their opacity (straight alpha, color unchanged).
    vr.until_rgba("background is transparent", (790.0, 590.0), None, 0, 2);
    vr.until_rgba("opaque", at("opaque"), Some([0x2e, 0xcc, 0x71]), 255, 8);
    vr.until_rgba("50%", at("half"), Some([0, 0, 0]), 128, 8);
    vr.until_rgba("25%", at("faint"), Some([255, 255, 255]), 64, 8);

    // a translucent button can also be pressed with the laser.
    click(page, vr, "glass-button");
    page.until("pressed the translucent button", |s| {
        s["state"]["clicks"] == 1
    });

    // Close: closes only this panel.
    press(page, vr, "close");
    let deadline = Instant::now() + common::TIMEOUT;
    while page.app.get_webview_window(&label).is_some() {
        assert!(Instant::now() < deadline, "window does not close");
        std::thread::sleep(Duration::from_millis(20));
    }
    let deadline = Instant::now() + common::TIMEOUT;
    while vr.texture_size(1) != Some((1, 1)) {
        assert!(
            Instant::now() < deadline,
            "panel does not become a transparent 1px after the out animation"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    page.label = "panel-0".into();
    (vr.panel, vr.resolution, vr.overlay) = (PANEL_POS, (640, 480), 0);
    assert_eq!(
        num(&page.probe(), &["state", "counts", "pointerdown"]),
        downs + 1.0,
        "the laser to the neighboring panel does not reach this panel (only once, for open-transparent)"
    );
}

/// point at the element's center, press and release the select button, and wait for click to arrive.
fn click(page: &mut Page, vr: &mut Vr, id: &str) {
    let clicks = num(&page.probe(), &["state", "counts", "click"]);
    press(page, vr, id);
    page.until(&format!("released {id}"), |s| {
        !s["state"]["counts"].is_object() || num(s, &["state", "counts", "click"]) > clicks
    });
}

/// point at the element's center, press and release the select button (don't wait for the page after release).
fn press(page: &mut Page, vr: &mut Vr, id: &str) {
    vr.point_at(page.center(id));
    page.until(&format!("hover over {id}"), |s| has(&s["hover"], id));
    vr.set("trigger/value", 1.0);
    page.until(&format!("pressed {id}"), |s| has(&s["active"], id));
    vr.set("trigger/value", 0.0);
}
