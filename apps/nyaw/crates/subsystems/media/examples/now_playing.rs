//! 再生中のメディアを OS から取得できるか検証する。`cargo run -p media --example now_playing [秒]`
//! （既定 5 秒。1 秒ごとに取得した内容を出力する）。

use std::time::Duration;

fn main() {
    let seconds = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(5u64);
    let media = media::Media::start();
    for _ in 0..seconds {
        std::thread::sleep(Duration::from_secs(1));
        match media.now() {
            None => println!("nothing playing"),
            Some(m) => println!(
                "{} — {} [{}] {} {:?}/{:?} app={} ({}) icon={} art={} backdrop={} light={:?} controls={:?}",
                m.title,
                m.artist,
                m.album,
                if m.playing { "playing" } else { "paused" },
                m.position_now(),
                m.duration,
                m.app.name,
                m.app.id,
                m.app.icon.is_some(),
                m.art
                    .as_ref()
                    .map_or("none".into(), |a| format!("{}x{}", a.width, a.height)),
                m.backdrop.is_some(),
                m.light,
                m.controls,
            ),
        }
    }
}
