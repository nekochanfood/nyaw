//! テスト用のループバックの「GitHub」。リリース一覧（API の JSON）とインストーラーを返す（`test-support` フィーチャー）。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

/// サーバーが返すファイル（パス → 内容）。テスト中に差し替え可能。
pub type Files = Arc<Mutex<HashMap<String, Vec<u8>>>>;

/// `fixture` が配置するインストーラーのファイル名と内容。
pub const INSTALLER: &str = "nyaw-2026.0.1-patch.2-setup.exe";
pub const BODY: &[u8] = b"MZ fake installer";

/// GitHub API レスポンスの形式（使用部分のみ）。`build` があればワークフロー定義どおりリリースノート末尾に注釈を追加する。
/// 名前付きバージョン（`-<名前>.<数>`）は、ワークフロー定義どおりプレリリースとする。
pub fn release(
    tag: &str,
    draft: bool,
    build: Option<u32>,
    assets: &[(&str, u64, Option<&str>)],
    base: &str,
) -> serde_json::Value {
    let marker = build.map_or(String::new(), |b| format!("\n\n<!-- nyaw-build: {b} -->"));
    serde_json::json!({
        "tag_name": tag,
        "name": tag,
        "body": format!("## What's Changed\n* {tag}{marker}"),
        "draft": draft,
        "prerelease": tag.contains('-'),
        "published_at": "2026-11-13T00:00:00Z",
        "html_url": format!("https://github.com/nekochanfood/nyaw/releases/tag/{tag}"),
        "assets": assets.iter().map(|(name, size, digest)| serde_json::json!({
            "name": name,
            "size": size,
            "browser_download_url": format!("{base}/{name}"),
            "digest": digest,
        })).collect::<Vec<_>>(),
    })
}

/// 下書き・インストーラーやダイジェストやビルド番号の欠けたリリースを含む一覧（API の JSON）。
pub fn varied(base: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!([
        // 下書きは除外する（ビルド番号が最大でも）。
        release(
            "v2026.0.1-patch.9",
            true,
            Some(999),
            &[("nyaw-2026.0.1-patch.9-setup.exe", 1, None)],
            base
        ),
        release(
            "v2026.0.1-patch.2",
            false,
            Some(655),
            &[
                ("phonon.dll", 1, None),
                ("nyaw-2026.0.1-patch.2-setup.exe", 5, Some("sha256:abc")),
            ],
            base
        ),
        // GitHub が SHA-256 を提供しないインストーラーは検証不可のため推奨しない（番号が最大でも）。
        release(
            "v2026.0.1-patch.4",
            false,
            Some(670),
            &[("nyaw-2026.0.1-patch.4-setup.exe", 5, None)],
            base
        ),
        // ビルド番号はあるがインストーラーが未添付（ビルド中）。
        release("v2026.0.1-patch.3", false, Some(660), &[], base),
        // バージョンはタグから取得（アセット名の記号は GitHub が変換する場合がある）。
        release(
            "v2026.0.1-ぽてと.1",
            false,
            Some(650),
            &[("nyaw-2026.0.1-....1-setup.exe", 5, Some("sha256:def"))],
            base
        ),
        // ビルド番号のない、更新機構導入前のリリース。
        release(
            "v2026.0.1",
            false,
            None,
            &[("nyaw-2026.0.1-setup.exe", 5, None)],
            base
        ),
        // 正式リリース（安定版チャンネルにも推奨する）。
        release(
            "v2026.0.2",
            false,
            Some(640),
            &[("nyaw-2026.0.2-setup.exe", 5, Some("sha256:123"))],
            base
        ),
    ]))
    .unwrap()
}

/// ローカル HTTP サーバー（ループバック）を起動し、その URL を返す。`files` のパスを返し、他は 404。
pub fn serve(files: Files) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
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

/// `bytes` の SHA-256（16 進）。
pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// サーバーを起動し、`/releases` に patch.1（ビルド 650）・patch.2（ビルド 655）の一覧と、patch.2 のインストーラー
/// （[`INSTALLER`]）を配置する。サーバーの URL と、差し替え用のファイルを返す。
pub fn fixture() -> (String, Files) {
    let files = Arc::new(Mutex::new(HashMap::new()));
    let base = serve(files.clone());
    let json = serde_json::to_vec(&serde_json::json!([
        release(
            "v2026.0.1-patch.2",
            false,
            Some(655),
            &[(
                INSTALLER,
                BODY.len() as u64,
                Some(&format!("sha256:{}", sha256(BODY))),
            )],
            &base
        ),
        release(
            "v2026.0.1-patch.1",
            false,
            Some(650),
            &[("nyaw-2026.0.1-patch.1-setup.exe", 1, None)],
            &base
        ),
    ]))
    .unwrap();
    let mut f = files.lock().unwrap();
    f.insert("/releases".into(), json);
    f.insert(format!("/{INSTALLER}"), BODY.to_vec());
    drop(f);
    (base, files)
}
