//! 通常のニャプリのロック（[`nyapp::NORMAL_NYAPPS_ENABLED`]）。どちらの状態でも流れ、ロック中は入口がすべて断ることを、
//! 外しているときはロックで断らないことを確かめる。ロックを掛けたら `cargo test -p nyapp -p dashboard` で確かめる
//! （ニャプリを動かすほかのテストは、ロック中は飛ばす）。

use std::path::Path;

use nyapp::{Installed, Manifest, installed, update, worker};

const DISABLED: &str = "nyapps are disabled";

fn app(path: &Path) -> Installed {
    let manifest =
        Manifest::parse("id = \"net.example.locktest\"\nname = \"t\"\nversion = \"1\"\napi = 1\n")
            .unwrap();
    Installed {
        manifest,
        path: path.to_owned(),
        icon: Default::default(),
    }
}

#[test]
fn every_entry_follows_the_lock() {
    // 無いパッケージ。ロックを外していればファイルを読みに行って失敗するだけで、何も書かない。
    let missing = std::env::temp_dir().join(format!("nyapp-lock-{}.nyapp", std::process::id()));
    let app = app(&missing);
    let start = worker::RemoteSandbox::start(&missing, &app.manifest.id, "ja").map(|_| ());
    let install = installed::install(&missing).map(|_| ());
    // 更新情報の無いものは、ロックを外していれば確かめずに `Ok(None)`。
    let update = update::update(&app).map(|_| ());

    if nyapp::NORMAL_NYAPPS_ENABLED {
        for (entry, result) in [("start", start), ("install", install), ("update", update)] {
            assert!(
                result.as_ref().err().is_none_or(|e| e.to_string() != DISABLED),
                "{entry} はロックで断らない: {result:?}"
            );
        }
    } else {
        for (entry, result) in [("start", start), ("install", install), ("update", update)] {
            assert_eq!(
                result.err().map(|e| e.to_string()).as_deref(),
                Some(DISABLED),
                "{entry}"
            );
        }
        assert!(installed::scan().is_empty(), "一覧に出さない");
        // ワーカーの入口は OS の制限を掛ける前に断る（外しているときに呼ぶとこのプロセスを縛るので呼ばない）。
        assert_eq!(worker::main().unwrap_err().to_string(), DISABLED);
    }
}
