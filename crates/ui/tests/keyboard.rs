//! 本番の描画ループで、キーボード画面を追加した後も描画と操作が進むか確認する。
mod common;

use common::SharedMock;
use kernel::{Kernel, Pose, VrBackend};
use std::{
    process::ExitCode,
    sync::{
        Arc, Mutex,
        mpsc::{Sender, channel},
    },
    time::{Duration, Instant},
};
use ui::{PanelControl, PanelSpec};

fn save(texture: &kernel::vr::mock::MockTexture, name: &str) {
    let mut bmp = vec![0u8; 54];
    bmp[..2].copy_from_slice(b"BM");
    bmp[2..6].copy_from_slice(&(54 + texture.rgba.len() as u32).to_le_bytes());
    bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
    bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
    bmp[18..22].copy_from_slice(&texture.width.to_le_bytes());
    bmp[22..26].copy_from_slice(&(-(texture.height as i32)).to_le_bytes());
    bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
    bmp[28..30].copy_from_slice(&32u16.to_le_bytes());
    for p in texture.rgba.as_chunks::<4>().0 {
        bmp.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join(name), bmp).unwrap();
}

fn wait(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready() {
        assert!(Instant::now() < deadline, "keyboard scenario timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn on_app<R: Send + 'static>(
    remote: &Sender<ui::Remote>,
    f: impl FnOnce(&mut gpui::App) -> R + Send + 'static,
) -> R {
    let (tx, rx) = channel();
    remote
        .send(Box::new(move |cx| tx.send(f(cx)).unwrap()))
        .unwrap();
    rx.recv_timeout(Duration::from_secs(10))
        .expect("UI thread stopped responding")
}

fn main() -> ExitCode {
    if !common::should_run("keyboard") {
        return ExitCode::SUCCESS;
    }
    let mock = SharedMock::default();
    let observer = mock.0.clone();
    let kernel = Kernel::with_backend(
        Box::new(mock.clone()) as Box<dyn VrBackend>,
        kernel::Config {
            pointer_mode: Some(kernel::PointerMode::Mouse),
            pointer_smoothing: Some(0.),
            ..Default::default()
        },
    );
    let panels = vec![
        PanelSpec::new(
            Pose::new(common::PANEL_POS, Default::default()),
            |id, _, _| testing::Testing::new(id),
        )
        .sized(testing::TESTING_SIZE, testing::TESTING_PX),
    ];
    let (remote, rx) = channel::<ui::Remote>();
    let result: Arc<Mutex<Option<bool>>> = Arc::default();
    let out = result.clone();
    std::thread::spawn(move || {
        let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            std::thread::sleep(Duration::from_secs(1));
            on_app(&remote, |cx| {
                cx.default_global::<PanelControl>()
                    .spawns
                    .push(testing::KeyboardTest::spec());
            });
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let dimensions = observer
                    .lock()
                    .unwrap()
                    .overlays
                    .get(1)
                    .and_then(|p| p.texture.as_ref())
                    .map(|t| (t.width, t.height));
                if dimensions == Some((1600, 1100)) {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "keyboard never produced a frame: {dimensions:?}"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
            assert_eq!(on_app(&remote, |cx| cx.global::<PanelControl>().open), 2);
            on_app(&remote, |cx| {
                ui::keyboard::show_keyboard(1, ui::keyboard::KeyboardMode::Persistent, cx)
            });
            wait(|| {
                observer
                    .lock()
                    .unwrap()
                    .overlays
                    .get(2)
                    .is_some_and(|p| p.texture.is_some())
            });
            // 初回のローディング画像ではなく、実際のキーが届くまで待つ。
            std::thread::sleep(Duration::from_millis(500));
            let pose = {
                let m = observer.lock().unwrap();
                save(m.overlays[1].texture.as_ref().unwrap(), "keyboard-test.bmp");
                let keyboard = &m.overlays[2];
                let texture = keyboard.texture.as_ref().unwrap();
                assert_eq!((texture.width, texture.height), (1552, 552));
                save(texture, "keyboard.bmp");
                keyboard.pose
            };
            assert!(!on_app(&remote, |cx| cx.global::<PanelControl>().panels
                [&2]
                .status
                .locked));
            let mut hand = common::Vr::new(mock);
            let key = pose.transform_point(kernel::vr::Vec3::new(
                184. / 1552. - 0.5,
                (0.5 - 352. / 552.) * 552. / 1552.,
                0.,
            ));
            hand.aim.orientation = kernel::vr::Quat::from_rotation_arc(
                kernel::vr::Vec3::NEG_Z,
                (key - hand.aim.position).normalize(),
            );
            hand.set("trigger/value", 1.);
            wait(|| {
                let m = observer.lock().unwrap();
                let frame = m.overlays[2].texture.as_ref().unwrap();
                frame.pixel(174, 340)[0] > frame.pixel(160, 340)[0].saturating_add(5)
            });
            save(
                observer.lock().unwrap().overlays[2]
                    .texture
                    .as_ref()
                    .unwrap(),
                "keyboard-ripple.bmp",
            );
            hand.set("trigger/value", 0.);
            hand.aim.orientation = kernel::vr::Quat::from_rotation_arc(
                kernel::vr::Vec3::NEG_Z,
                (pose.position - hand.aim.position).normalize(),
            );
            hand.set("squeeze/value", 1.);
            wait(|| {
                on_app(&remote, |cx| {
                    cx.global::<PanelControl>().panels[&2].status.grabbed
                })
            });
            hand.aim.position.y += 0.15;
            hand.send();
            wait(|| {
                observer.lock().unwrap().overlays[2]
                    .pose
                    .position
                    .distance(pose.position)
                    > 0.1
            });
            hand.set("squeeze/value", 0.);
            wait(|| {
                !on_app(&remote, |cx| {
                    cx.global::<PanelControl>().panels[&2].status.grabbed
                })
            });
        }))
        .is_ok();
        *out.lock().unwrap() = Some(ok);
        common::quit(&remote, ok);
    });
    ui::run_with(
        kernel,
        panels,
        None,
        ui::Hooks {
            remote: Some(rx),
            ..Default::default()
        },
    );
    common::exit_code("keyboard", &result)
}
