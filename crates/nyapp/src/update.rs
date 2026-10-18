//! 更新。AppImage の Update Information と同じく、マニフェストの `[update]`（[`crate::manifest::UpdateInfo`]）が
//! 最新版を知らせるフィードの場所を持ち、nyaw がそれを読んで新しければ落として入れ替える。
//!
//! フィード（TOML）:
//! ```toml
//! id = "net.bettaworx.clock"
//! version = "2026.1.0"
//! package = "net.bettaworx.clock-2026.1.0.nyapp"   # URL。相対ならフィードの場所から
//! sha256 = "…"                                     # パッケージの SHA-256（16 進）。必須
//! ```
//!
//! 落としたパッケージは、SHA-256・ID・バージョンがフィードのとおりで、入っているものより新しいときだけ入れる。
//! どれもネットワークとファイルを使うので、別スレッドで呼ぶ。

use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{Error, Installed, installed, package};

/// フィード・パッケージの大きさの上限。
const MAX_FEED: u64 = 64 << 10;
const MAX_PACKAGE: u64 = 64 << 20;
/// 1 回の取得にかけてよい時間。
const TIMEOUT: Duration = Duration::from_secs(30);

/// 最新版の知らせ。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feed {
    pub id: String,
    pub version: String,
    pub package: String,
    pub sha256: String,
}

/// 取りに行ってよい URL か: `https`、または手元の確認用にループバック（`localhost` / `127.0.0.1` / `[::1]`）の `http`。
pub fn check_url(url: &str) -> Result<(), Error> {
    let host = |rest: &str| {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        match host.strip_prefix('[') {
            Some(v6) => v6.split(']').next().unwrap_or_default().to_owned(),
            None => host.split(':').next().unwrap_or_default().to_owned(),
        }
    };
    let ok = match url.split_once("://") {
        Some(("https", rest)) => !host(rest).is_empty(),
        Some(("http", rest)) => matches!(host(rest).as_str(), "localhost" | "127.0.0.1" | "::1"),
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(Error::new(format!(
            "update url must be https (or http on loopback): {url}"
        )))
    }
}

/// `b` が `a` より新しい版か。`.` と `-` で区切った部分を、数なら数として、そうでなければ文字として前から比べる
/// （`2026.10.0` > `2026.9.3`）。
pub fn newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<(u64, String)> {
        v.split(['.', '-'])
            .map(|p| p.parse().map_or((0, p.to_owned()), |n| (n, String::new())))
            .collect()
    };
    parts(b) > parts(a)
}

/// `app` の更新を確かめる。新しい版があればそのフィード、更新情報が無いか最新なら `None`。
pub fn check(app: &Installed) -> Result<Option<Feed>, Error> {
    let Some(info) = &app.manifest.update else {
        return Ok(None);
    };
    let feed: Feed = toml::from_str(
        &String::from_utf8(fetch(&info.url, MAX_FEED)?)
            .map_err(|_| Error::new("the update feed is not UTF-8"))?,
    )?;
    if feed.id != app.manifest.id {
        return Err(Error::new(format!(
            "the update feed is for {}, not {}",
            feed.id, app.manifest.id
        )));
    }
    Ok(newer(&app.manifest.version, &feed.version).then_some(feed))
}

/// `app` に新しい版があれば落として入れ替える。入れ替えたら新しいもの、最新なら `None`。
pub fn update(app: &Installed) -> Result<Option<Installed>, Error> {
    let dir = installed::dir().ok_or_else(|| Error::new("no data directory"))?;
    update_into(&dir, app)
}

pub(crate) fn update_into(dir: &Path, app: &Installed) -> Result<Option<Installed>, Error> {
    let Some(feed) = check(app)? else {
        return Ok(None);
    };
    let base = app
        .manifest
        .update
        .as_ref()
        .map(|u| u.url.as_str())
        .unwrap_or_default();
    let bytes = fetch(&resolve(base, &feed.package)?, MAX_PACKAGE)?;
    let digest: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if !digest.eq_ignore_ascii_case(feed.sha256.trim()) {
        return Err(Error::new(format!(
            "the downloaded package does not match sha256 {}",
            feed.sha256
        )));
    }
    fs::create_dir_all(dir)?;
    let download = installed::staging(dir, &app.manifest.id, "download");
    fs::write(&download, &bytes)?;
    let result = (|| {
        let manifest = package::read_manifest(&download)?;
        if manifest.id != feed.id || manifest.version != feed.version {
            return Err(Error::new(format!(
                "the downloaded package is {} {}, not {} {}",
                manifest.id, manifest.version, feed.id, feed.version
            )));
        }
        installed::install_into(dir, &download)
    })();
    let _ = fs::remove_file(&download);
    result.map(Some)
}

/// フィードの中のパッケージの場所を、フィードの URL `base` から見た絶対 URL にする（`/` 始まり・`//` 始まりも URL の規則どおり）。
fn resolve(base: &str, package: &str) -> Result<String, Error> {
    let base = url::Url::parse(base).map_err(|e| Error::new(format!("bad update url {base}: {e}")))?;
    base.join(package)
        .map(String::from)
        .map_err(|e| Error::new(format!("bad package url {package}: {e}")))
}

/// `url` を `max` バイトまで取る。
fn fetch(url: &str, max: u64) -> Result<Vec<u8>, Error> {
    check_url(url)?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .build()
        .into();
    let mut response = agent.get(url).call()?;
    let mut out = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(max + 1)
        .read_to_end(&mut out)?;
    if out.len() as u64 > max {
        return Err(Error::new(format!("{url} is larger than {max} bytes")));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    #[test]
    fn versions_compare_by_parts() {
        assert!(newer("2026.0.1", "2026.1.0"));
        assert!(newer("2026.9.3", "2026.10.0"), "数として比べる");
        assert!(newer("1.0", "1.0.1"));
        assert!(!newer("2026.1.0", "2026.1.0"));
        assert!(!newer("2026.1.0", "2026.0.9"));
    }

    #[test]
    fn only_https_or_loopback_http() {
        assert!(check_url("https://example.com/x.toml").is_ok());
        assert!(check_url("http://localhost:1234/x.toml").is_ok());
        assert!(check_url("http://[::1]:1234/x.toml").is_ok());
        assert!(check_url("http://127.0.0.1.example.com/x.toml").is_err());
        assert!(check_url("http://user@example.com/x.toml").is_err());
        assert!(check_url("file:///etc/passwd").is_err());
        assert!(check_url("https://").is_err());
    }

    #[test]
    fn package_urls_resolve_against_the_feed() {
        let feed = "https://example.com/nyapps/clock/latest.toml";
        assert_eq!(
            resolve(feed, "clock-2.nyapp").unwrap(),
            "https://example.com/nyapps/clock/clock-2.nyapp"
        );
        assert_eq!(
            resolve(feed, "/releases/clock.nyapp").unwrap(),
            "https://example.com/releases/clock.nyapp"
        );
        assert_eq!(
            resolve(feed, "https://cdn.example.com/c.nyapp").unwrap(),
            "https://cdn.example.com/c.nyapp"
        );
    }

    /// 手元の HTTP サーバー（ループバック）。`files` のパスを返し、ほかは 404。止めずに置いておく（テストの終わりで消える）。
    fn serve(files: Arc<Mutex<HashMap<String, Vec<u8>>>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let path = line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_owned();
                // ヘッダーは読み捨てる。
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) <= 2 {
                        break;
                    }
                }
                let body = files.lock().unwrap().get(&path).cloned();
                let mut stream = reader.into_inner();
                let (status, body) = match body {
                    Some(b) => ("200 OK", b),
                    None => ("404 Not Found", Vec::new()),
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(&body);
            }
        });
        format!("http://127.0.0.1:{}", addr.port())
    }

    fn manifest(version: &str, update: &str) -> String {
        format!(
            "id = \"net.bettaworx.clock\"\nname = \"Clock\"\nversion = \"{version}\"\napi = 1\n\n[update]\nurl = \"{update}\"\n"
        )
    }

    fn sha256(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    struct Fixture {
        root: PathBuf,
        dir: PathBuf,
        files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        feed_url: String,
        app: Installed,
    }

    /// 2026.0.1 が入っていて、サーバーには 2026.1.0 のパッケージがある。
    fn fixture(name: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!("nyapp-update-{name}-{}", std::process::id()));
        let dir = root.join("nyapps");
        fs::create_dir_all(&dir).unwrap();
        let files = Arc::new(Mutex::new(HashMap::new()));
        let feed_url = format!("{}/clock/latest.toml", serve(files.clone()));
        let src = root.join("old.nyapp");
        package::write(
            &src,
            &manifest("2026.0.1", &feed_url),
            b"old",
            &package::Icon::default(),
        )
        .unwrap();
        let app = installed::install_into(&dir, &src).unwrap();
        let new = root.join("new.nyapp");
        package::write(
            &new,
            &manifest("2026.1.0", &feed_url),
            b"new",
            &package::Icon::default(),
        )
        .unwrap();
        let new = fs::read(new).unwrap();
        let feed = format!(
            "id = \"net.bettaworx.clock\"\nversion = \"2026.1.0\"\npackage = \"clock-2026.1.0.nyapp\"\nsha256 = \"{}\"\n",
            sha256(&new)
        );
        let mut f = files.lock().unwrap();
        f.insert("/clock/latest.toml".into(), feed.into_bytes());
        f.insert("/clock/clock-2026.1.0.nyapp".into(), new);
        drop(f);
        Fixture {
            root,
            dir,
            files,
            feed_url,
            app,
        }
    }

    /// 新しい版があれば落として入れ替え、入れ替えた後は最新とみなす。
    #[test]
    fn updates_to_the_newer_version_from_the_feed() {
        let f = fixture("newer");
        assert_eq!(
            check(&f.app).unwrap().map(|feed| feed.version).as_deref(),
            Some("2026.1.0")
        );
        let updated = update_into(&f.dir, &f.app).unwrap().expect("updated");
        assert_eq!(updated.manifest.version, "2026.1.0");
        assert_eq!(package::read(&updated.path).unwrap().1, b"new");
        assert_eq!(
            update_into(&f.dir, &updated)
                .unwrap()
                .map(|i| i.manifest.version),
            None,
            "もう最新"
        );
        assert!(
            !f.dir.join("net.bettaworx.clock.download").exists(),
            "落としたものは片付ける"
        );
        fs::remove_dir_all(f.root).unwrap();
    }

    /// 落としたものがフィードの SHA-256 と違えば入れず、入っているものはそのまま。
    #[test]
    fn rejects_a_package_that_does_not_match_the_feed() {
        let f = fixture("tampered");
        f.files
            .lock()
            .unwrap()
            .insert("/clock/clock-2026.1.0.nyapp".into(), b"tampered".to_vec());
        assert!(update_into(&f.dir, &f.app).is_err());
        assert_eq!(package::read(&f.app.path).unwrap().0.version, "2026.0.1");
        fs::remove_dir_all(f.root).unwrap();
    }

    /// ほかのニャプリのフィード・届かないフィードはエラー。更新情報の無いものは確かめない。
    #[test]
    fn feed_errors_and_apps_without_update_information() {
        let f = fixture("errors");
        let other =
            "id = \"net.bettaworx.other\"\nversion = \"9\"\npackage = \"x\"\nsha256 = \"0\"\n";
        f.files
            .lock()
            .unwrap()
            .insert("/clock/latest.toml".into(), other.as_bytes().to_vec());
        assert!(check(&f.app).is_err(), "ほかのニャプリのフィード");
        f.files.lock().unwrap().clear();
        assert!(check(&f.app).is_err(), "404");
        let mut plain = f.app.clone();
        plain.manifest.update = None;
        assert_eq!(check(&plain).unwrap(), None);
        let _ = f.feed_url;
        fs::remove_dir_all(f.root).unwrap();
    }
}
