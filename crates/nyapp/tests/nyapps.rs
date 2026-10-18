//! 試作のニャプリ（`nyapps/`）を本物の wasm としてサンドボックスに載せ、出来事を送って確かめる。
//! wasm へのビルドが要るので常には流さない: `cargo test -p nyapp --test nyapps -- --ignored`
//! （`rustup target add wasm32-unknown-unknown` が要る）。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;

use nyapp::{Manifest, Sandbox, package};
use nyapp_sdk::{Event, Node};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

/// `nyapps/` を一度だけビルドする。
fn build() {
    static BUILD: Once = Once::new();
    BUILD.call_once(|| {
        let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
            .current_dir(root().join("nyapps"))
            .status()
            .unwrap();
        assert!(status.success(), "nyapps のビルド");
    });
}

fn load(name: &str) -> (Sandbox, Node) {
    build();
    let dir = root().join("nyapps").join(name);
    let manifest =
        Manifest::parse(&std::fs::read_to_string(dir.join("manifest.toml")).unwrap()).unwrap();
    let wasm = std::fs::read(
        root()
            .join("nyapps/target/wasm32-unknown-unknown/release")
            .join(format!("{name}.wasm")),
    )
    .unwrap();
    // 保存領域はテストごとの一時フォルダ（本物の appdata には触れない）。
    let appdata = std::env::temp_dir().join(format!("nyapps-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&appdata);
    Sandbox::load(&manifest, &wasm, Some(appdata), "ja").unwrap()
}

fn click(sandbox: &mut Sandbox, id: &str) -> Node {
    sandbox.event(&Event::Click { id: id.into() }).unwrap()
}

/// 画面の木の文字をすべて（前から順に）。
fn texts(node: &Node) -> Vec<String> {
    let json = serde_json::to_value(node).unwrap();
    let mut out = Vec::new();
    fn walk(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(m) => {
                if m.get("type").and_then(|t| t.as_str()) == Some("text")
                    && let Some(t) = m.get("text").and_then(|t| t.as_str())
                {
                    out.push(t.to_owned());
                }
                m.values().for_each(|v| walk(v, out));
            }
            serde_json::Value::Array(a) => a.iter().for_each(|v| walk(v, out)),
            _ => {}
        }
    }
    walk(&json, &mut out);
    out
}

#[test]
#[ignore]
fn calculator_adds_and_divides() {
    let (mut calc, view) = load("calculator");
    assert!(texts(&view).contains(&"0".to_owned()));
    for key in ["key-1", "key-2", "key-+", "key-3"] {
        click(&mut calc, key);
    }
    let view = click(&mut calc, "key-=");
    assert!(
        texts(&view).contains(&"15".to_owned()),
        "{:?}",
        texts(&view)
    );
    for key in ["key-c", "key-1", "key-/", "key-0"] {
        click(&mut calc, key);
    }
    let view = click(&mut calc, "key-=");
    assert!(texts(&view).contains(&"Error".to_owned()));
}

/// サンドボックス診断の確かめがすべて通る（権限の有無・保存領域の上限・メモリなど）。
#[test]
#[ignore]
fn sandbox_probes_all_pass() {
    let (mut probe, view) = load("sandbox");
    let all = texts(&view);
    assert!(
        !all.contains(&"✗".to_owned()),
        "通らない確かめがある: {all:?}"
    );
    let summary = all.iter().find(|t| t.starts_with("ホスト関数")).unwrap();
    let (passed, total) = summary.rsplit_once(' ').unwrap().1.split_once('/').unwrap();
    assert_eq!(passed, total, "{summary}");
    assert_eq!(probe.host().beeps, 1, "権限 audio の音を 1 回頼んだ");
    // 周期を頼むとホストに届く。
    click(&mut probe, "ticker-toggle");
    assert_eq!(probe.host().ticker.take(), Some(100));
    let view = probe.event(&Event::Tick).unwrap();
    assert!(texts(&view).contains(&"ticks = 1".to_owned()));
}

/// 意図的なクラッシュはどれもエラー（trap）として返り、ホストは落ちず、次のニャプリもそのまま動く。
#[test]
#[ignore]
fn intentional_crashes_are_contained() {
    for kind in ["crash-panic", "crash-loop", "crash-memory", "crash-stack"] {
        let (mut probe, _) = load("sandbox");
        let result = probe.event(&Event::Click { id: kind.into() });
        let error = result.expect_err(kind).to_string();
        println!("{kind}: {error}");
        // 落ちたニャプリはもう動かさないが、別に起こしたものは動く。
        let (mut fresh, _) = load("sandbox");
        fresh.event(&Event::Tick).unwrap();
    }
}

#[test]
#[ignore]
fn clock_stopwatch_asks_for_fine_ticks() {
    let (mut clock, view) = load("clock");
    assert!(
        texts(&view).iter().any(|t| t.contains('年')),
        "言語は ja（日付が日本語）"
    );
    clock.host().ticker.take();
    click(&mut clock, "tab-stopwatch");
    click(&mut clock, "watch-start");
    assert_eq!(clock.host().ticker.take(), Some(50));
    click(&mut clock, "watch-start");
    assert_eq!(clock.host().ticker.take(), Some(0), "止めたら周期も止める");
}

#[test]
#[ignore]
fn eyes_follow_the_pointer() {
    let (mut eyes, before) = load("eyes");
    let after = eyes
        .event(&Event::Pointer {
            id: "eyes".into(),
            x: 600.0,
            y: 0.0,
        })
        .unwrap();
    assert_ne!(before, after, "黒目が動く");
}

/// パッケージにまとめると、SVG・PNG のアイコンは画像になり、アイコンの無いもの（サンドボックス診断）は代わりを描く。
#[test]
#[ignore]
fn packaged_icons_decode_or_fall_back() {
    build();
    let out = std::env::temp_dir().join(format!("nyapps-icons-{}", std::process::id()));
    std::fs::create_dir_all(&out).unwrap();
    for (name, has_icon) in [
        ("calculator", true),
        ("clock", true),
        ("eyes", true),
        ("sandbox", false),
    ] {
        let dir = root().join("nyapps").join(name);
        let icon = ["icon.svg", "icon.png"].iter().find_map(|f| {
            let bytes = std::fs::read(dir.join(f)).ok()?;
            Some(if f.ends_with("svg") {
                package::Icon::Svg(bytes)
            } else {
                package::Icon::Png(bytes)
            })
        });
        let wasm = std::fs::read(
            root()
                .join("nyapps/target/wasm32-unknown-unknown/release")
                .join(format!("{name}.wasm")),
        )
        .unwrap();
        let path = out.join(format!("{name}.nyapp"));
        let manifest = std::fs::read_to_string(dir.join("manifest.toml")).unwrap();
        package::write(&path, &manifest, &wasm, icon.as_ref()).unwrap();
        let (_, icon) = package::read_listing(&path).unwrap();
        assert_eq!(icon.is_some(), has_icon, "{name}");
        assert_eq!(
            icon.as_ref().and_then(nyapp::icon::decode).is_some(),
            has_icon,
            "{name} のアイコンが読める"
        );
    }
    std::fs::remove_dir_all(out).unwrap();
}

/// ニャプリはそれぞれ別のサンドボックス（メモリ・状態・保存領域）で動き、互いに見えない（マルチテナント）。
/// 片方が落ちても、もう片方はそのまま動く。
#[test]
#[ignore]
fn each_nyapp_runs_in_its_own_sandbox() {
    let (mut a, _) = load("sandbox");
    let (mut b, _) = load("sandbox");
    let switch_of = |node: &Node| {
        serde_json::to_string(node)
            .unwrap()
            .contains(r#""id":"probe-switch","on":true"#)
    };
    let view_a = a
        .event(&Event::Toggle {
            id: "probe-switch".into(),
            on: true,
        })
        .unwrap();
    assert!(switch_of(&view_a));
    let view_b = b.event(&Event::Tick).unwrap();
    assert!(!switch_of(&view_b), "a の状態は b に見えない");
    assert!(
        a.event(&Event::Click {
            id: "crash-panic".into()
        })
        .is_err()
    );
    let view_b = b.event(&Event::Tick).unwrap();
    assert!(
        texts(&view_b).contains(&"ticks = 2".to_owned()),
        "a が落ちても b は動く"
    );
}
