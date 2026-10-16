//! 本番の描画ループで、キーボード画面を追加した後も描画と操作が進むか確認する。
mod common;

use common::SharedMock;
use kernel::{Kernel, Pose, Vec2, VrBackend};
use std::{
    process::ExitCode,
    sync::{
        Arc, Mutex,
        mpsc::{Sender, channel},
    },
    time::{Duration, Instant},
};
use ui::{PanelControl, PanelSpec};

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
    let kernel = Kernel::with_backend(Box::new(mock) as Box<dyn VrBackend>, Default::default());
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
                cx.default_global::<PanelControl>().spawns.push(
                    PanelSpec::spawn(0, |id, _, cx| testing::KeyboardTest::new(id, cx))
                        .sized(Vec2::new(1.6, 1.1), (1600, 1100)),
                );
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
