//! nyaw のロゴのどのフレームを描いても、tiny-skia が警告を出さないこと（長さのない線を描かせない）。
//! ログの受け口はプロセスに 1 つなので、ほかのテストと分ける。

use std::sync::Mutex;

use lottie::Animation;

static WARNINGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Collect;

impl log::Log for Collect {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            WARNINGS.lock().unwrap().push(record.args().to_string());
        }
    }

    fn flush(&self) {}
}

#[test]
fn rendering_every_frame_logs_no_warnings() {
    log::set_logger(&Collect).unwrap();
    log::set_max_level(log::LevelFilter::Warn);
    for json in [
        include_str!("../../../../../assets/animations/logo-full/logo-full-light.json"),
        include_str!("../../../../../assets/animations/logo-full/logo-full-dark.json"),
    ] {
        let anim = Animation::from_json(json).unwrap();
        for frame in anim.in_point as u32..anim.out_point as u32 {
            anim.render(f64::from(frame), 401, 199).unwrap();
        }
    }
    let warnings = WARNINGS.lock().unwrap();
    assert!(
        warnings.is_empty(),
        "{} warnings: {:?}",
        warnings.len(),
        warnings.first()
    );
}
