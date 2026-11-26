//! 再生中のメディアを OS から取得し、操作できるか検証する。
//! `cargo run -p media --example now_playing [秒] [操作…]`（既定 5 秒。1 秒ごとに取得した内容を出力する）。
//! 操作は 2 秒ごとに 1 つずつ順に送る: `switch`、`playpause`、`next`、`previous`、`seek=<秒>`、`raise`、`wait`（何もしない）。

use std::time::Duration;

use media::Command;

fn command(name: &str) -> Option<Command> {
    match name {
        "switch" => Some(Command::Switch(true)),
        "playpause" => Some(Command::PlayPause),
        "next" => Some(Command::Next),
        "previous" => Some(Command::Previous),
        "raise" => Some(Command::Raise),
        "wait" => None,
        _ => {
            let seconds = name.strip_prefix("seek=").and_then(|s| s.parse().ok());
            let seconds = seconds.unwrap_or_else(|| panic!("unknown command: {name}"));
            Some(Command::Seek(Duration::from_secs_f64(seconds)))
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let seconds = args.next().and_then(|s| s.parse().ok()).unwrap_or(5u64);
    let commands: Vec<(String, Option<Command>)> =
        args.map(|name| (name.clone(), command(&name))).collect();
    let mut commands = commands.into_iter();
    let media = media::Media::start();
    for second in 0..seconds {
        std::thread::sleep(Duration::from_secs(1));
        match media.now() {
            None => println!("nothing playing"),
            Some(m) => println!(
                "{} — {} [{}] {} {:.1?}/{:.1?} sessions={} app={} ({}) icon={} art={} backdrop={} light={:?} controls={:?}",
                m.title,
                m.artist,
                m.album,
                if m.playing { "playing" } else { "paused" },
                m.position_now(),
                m.duration,
                m.sessions,
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
        if second % 2 == 1
            && let Some((name, command)) = commands.next()
        {
            println!("> {name}");
            if let Some(command) = command {
                media.send(command);
            }
        }
    }
}
