//! E2E for updates on the Settings screen (`settings-update`), against a loopback "GitHub": checking finds the newer
//! build, the page shows its version and release notes (Markdown), and only pressing the update button downloads the
//! installer and starts it. Opening from the dashboard's button (`updater::reveal`) lands on this category.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use gpui::TestAppContext;
use testing::Settings;
use testkit::{Driver, NoVrKit};

/// Index of the updates category in the sidebar.
const UPDATE: usize = 8;
const INSTALLER: &str = "nyaw-2026.0.1-patch.2-setup.exe";
const BODY: &[u8] = b"MZ fake installer";
/// `BODY` の SHA-256（GitHub がアセットの `digest` で知らせるもの。無いリリースは勧めない）。
const BODY_SHA256: &str = "9f7d203d2e9d47df255d63188d247ec305d7f3f9d40b9f1270574978cee3d1dd";

/// A loopback "GitHub": `/releases` lists an older build (650) and a newer one (655, with release notes), each build
/// number in the hidden note the release workflow appends, and the newer one's installer is downloadable.
/// Returns the list's URL.
fn github() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let notes = "## What's Changed\\n* feat: updates by @someone in https://github.com/nekochanfood/nyaw/pull/83\\n* fix: **bold** and `code`\\n\\n<!-- nyaw-build: 655 -->";
    let list = format!(
        r#"[
          {{"tag_name":"v2026.0.1-patch.2","name":"v2026.0.1-patch.2","body":"{notes}","draft":false,
            "published_at":"2026-11-13T00:00:00Z","html_url":"https://github.com/nekochanfood/nyaw/releases/tag/v2026.0.1-patch.2",
            "assets":[{{"name":"{INSTALLER}","size":{size},"browser_download_url":"{base}/{INSTALLER}","digest":"sha256:{BODY_SHA256}"}}]}},
          {{"tag_name":"v2026.0.1-patch.1","name":null,"body":"<!-- nyaw-build: 650 -->","draft":false,"published_at":null,
            "html_url":"https://github.com/nekochanfood/nyaw/releases/tag/v2026.0.1-patch.1",
            "assets":[{{"name":"nyaw-2026.0.1-patch.1-setup.exe","size":1,"browser_download_url":"{base}/x","digest":"sha256:{BODY_SHA256}"}}]}}
        ]"#,
        size = BODY.len()
    );
    let files: HashMap<String, Vec<u8>> = [
        ("/releases".to_owned(), list.into_bytes()),
        (format!("/{INSTALLER}"), BODY.to_vec()),
    ]
    .into();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            let path = line
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_owned();
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).unwrap_or(0) <= 2 {
                    break;
                }
            }
            let (status, body) = match files.get(&path) {
                Some(b) => ("200 OK", b.clone()),
                None => ("404 Not Found", Vec::new()),
            };
            let mut stream = reader.into_inner();
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(&body);
        }
    });
    format!("{base}/releases")
}

static LAUNCHED: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Stands in for running the installer (the real one would replace this test binary).
fn launch(path: &Path) -> Result<(), String> {
    *LAUNCHED.lock().unwrap() = Some(path.to_owned());
    Ok(())
}

/// The running build is 650; the "GitHub" has 655. Nothing is fetched or installed until the buttons are pressed.
#[gpui::test]
fn check_shows_the_new_release_with_its_notes_and_installs_only_on_request(
    cx: &mut TestAppContext,
) {
    let source = github();
    let dir = std::env::temp_dir().join(format!("nyaw-e2e-update-{}", std::process::id()));
    let download_dir = dir.clone();
    let mut kit = NoVrKit::new(cx, move |_, cx| {
        cx.set_global(updater::Updater {
            build: 650,
            source,
            dir: download_dir,
            launch,
            ..Default::default()
        });
        Settings::new(0)
    });
    kit.click("settings-update");
    assert_eq!(kit.read(|v| v.category), UPDATE);
    let (current, status) = (kit.bounds("update-current"), kit.bounds("update-status"));
    assert!(
        current.bottom() <= status.top(),
        "the current version, then the status"
    );

    kit.click("update-check").step().step();
    // The newer release: its version and the update button, then the release notes rendered as Markdown.
    let (version, install, notes) = (
        kit.bounds("update-version"),
        kit.bounds("update-install"),
        kit.bounds("update-notes"),
    );
    assert!(status.bottom() <= version.top());
    assert!(
        version.right() <= install.left(),
        "the button sits right of the version"
    );
    assert!(install.bottom() <= notes.top(), "notes below");
    let (heading, list, item) = (
        kit.bounds("update-notes-0"),
        kit.bounds("update-notes-1"),
        kit.bounds("update-notes-1-1-0"),
    );
    assert!(heading.bottom() <= list.top());
    assert!(
        heading.size.height > item.size.height,
        "the heading is larger than a list item"
    );
    let content = kit.bounds("settings-content");
    assert!(
        item.right() <= content.right(),
        "notes stay inside the page"
    );
    assert_eq!(
        *LAUNCHED.lock().unwrap(),
        None,
        "finding an update installs nothing"
    );

    kit.click("update-install").step().step();
    let launched = LAUNCHED
        .lock()
        .unwrap()
        .clone()
        .expect("the installer was started");
    assert_eq!(launched, dir.join(INSTALLER));
    assert_eq!(std::fs::read(&launched).unwrap(), BODY);
    kit.bounds("update-installing");
    let _ = std::fs::remove_dir_all(dir);
}

/// The dashboard's update button asks for the updates category; the Settings screen opens on it.
#[gpui::test]
fn opening_from_the_dashboard_button_lands_on_updates(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, cx| {
        updater::reveal(cx);
        Settings::new(0)
    });
    kit.step();
    assert_eq!(kit.read(|v| v.category), UPDATE);
    kit.bounds("update-check");
    // It is taken once: choosing another category afterwards stays there.
    kit.click("settings-about").step();
    assert_eq!(kit.read(|v| v.category), UPDATE + 1);
}

/// Against the real GitHub, run by hand: pretending to be build `NYAW_UPDATE_FROM`, the Settings screen finds the newest
/// release, and the update button downloads its installer and really starts it. When this test exits, the installer
/// replaces the user's nyaw without a wizard or UAC and starts it again with `--novr`.
/// `NYAW_UPDATE_FROM=<build> cargo test -p testkit --test scenarios -- update::real --ignored --nocapture`
#[gpui::test]
#[ignore = "talks to GitHub and installs nyaw for real"]
fn real_github_release_installs_on_request(cx: &mut TestAppContext) {
    let from: u32 = std::env::var("NYAW_UPDATE_FROM")
        .ok()
        .and_then(|b| b.parse().ok())
        .expect("NYAW_UPDATE_FROM=<build to update from>");
    let mut kit = NoVrKit::new(cx, move |_, cx| {
        cx.set_global(updater::Updater {
            build: from,
            launch: |path| updater::start_installer(path, ["--novr".to_owned()]),
            ..Default::default()
        });
        Settings::new(0)
    });
    kit.click("settings-update")
        .click("update-check")
        .step()
        .step();
    kit.bounds("update-version");
    kit.bounds("update-notes-0");
    kit.click("update-install").step().step();
    kit.bounds("update-installing");
}
