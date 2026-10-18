//! マニフェスト（パッケージの `manifest.toml`）。
//!
//! ```toml
//! id = "net.bettaworx.calculator"   # 逆ドメイン表記。パッケージのファイル名も `<id>.nyapp`
//! name = "Calculator"
//! version = "2026.0.1"
//! api = 1                            # nyapp_sdk::API
//! permissions = ["audio"]            # Permission
//!
//! [names]                            # 言語ごとの名前（無ければ name）
//! ja = "電卓"
//!
//! [update]                           # 任意。更新情報（AppImage の Update Information に当たる。crate::update）
//! url = "https://example.com/nyapps/net.bettaworx.calculator/latest.toml"
//! ```
//!
//! アイコンはマニフェストではなく、パッケージの `icon.png`（[`crate::package::ICON`]）に入れる。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Error;

/// ID の長さの上限。
const MAX_ID: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub names: BTreeMap<String, String>,
    pub version: String,
    pub api: u32,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    /// 更新情報（任意）。無ければ nyaw は更新を確かめない。
    #[serde(default)]
    pub update: Option<UpdateInfo>,
}

/// 更新情報: 最新版を知らせるフィード（[`crate::update::Feed`]）の場所。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateInfo {
    /// フィードの URL。`https`（手元の確認用に、ループバックだけは `http` も）。
    pub url: String,
}

/// 権限。マニフェストに書いたものだけが使える（インストールした時点で認める）。知らない名前のマニフェストは読まない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// パネルから知らせの音を鳴らす（`host::beep`）。
    Audio,
    /// 再生中のメディアのタイトルを読む（`host::now_playing`）。
    Media,
}

impl Permission {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "audio" => Some(Self::Audio),
            "media" => Some(Self::Media),
            _ => None,
        }
    }
}

impl Manifest {
    /// `manifest.toml` を読み、ID・ABI の版・名前を確かめる。
    pub fn parse(text: &str) -> Result<Self, Error> {
        let m: Manifest = toml::from_str(text)?;
        if !valid_id(&m.id) {
            return Err(Error::new(format!(
                "id `{}` is not reverse-domain notation (e.g. net.bettaworx.calculator)",
                m.id
            )));
        }
        if m.api != nyapp_sdk::API {
            return Err(Error::new(format!(
                "api {} is not supported (this nyaw speaks {})",
                m.api,
                nyapp_sdk::API
            )));
        }
        if m.name.trim().is_empty() {
            return Err(Error::new("name is empty"));
        }
        if let Some(update) = &m.update {
            crate::update::check_url(&update.url)?;
        }
        Ok(m)
    }

    /// 言語 `lang`（`en` / `ja`）での名前。
    pub fn name_in(&self, lang: &str) -> &str {
        self.names.get(lang).unwrap_or(&self.name)
    }

    pub fn permits(&self, permission: Permission) -> bool {
        self.permissions.contains(&permission)
    }
}

/// 逆ドメイン表記か: `.` で区切った 2 つ以上の部分で、どれも英小文字で始まり英小文字・数字・`_` だけ。
pub fn valid_id(id: &str) -> bool {
    let mut parts = 0;
    let ok = id.len() <= MAX_ID
        && id.split('.').all(|part| {
            parts += 1;
            part.starts_with(|c: char| c.is_ascii_lowercase())
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        });
    ok && parts >= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    const CALCULATOR: &str = r#"
id = "net.bettaworx.calculator"
name = "Calculator"
version = "2026.0.1"
api = 1
permissions = ["audio"]

[names]
ja = "電卓"
"#;

    #[test]
    fn reads_a_manifest_and_its_localized_name() {
        let m = Manifest::parse(CALCULATOR).unwrap();
        assert_eq!(m.id, "net.bettaworx.calculator");
        assert_eq!(m.name_in("ja"), "電卓");
        assert_eq!(m.name_in("en"), "Calculator");
        assert!(m.permits(Permission::Audio));
        assert!(!m.permits(Permission::Media));
    }

    #[test]
    fn ids_must_be_reverse_domain() {
        for ok in ["net.bettaworx.clock", "com.example.app_2", "a.b"] {
            assert!(valid_id(ok), "{ok}");
        }
        for bad in [
            "clock",
            "net..clock",
            "Net.bettaworx.clock",
            "net.1clock",
            "net.bettaworx.clock.",
            "../x.y",
            "",
        ] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    /// 知らない権限・知らない項目・話せない ABI のマニフェストは読まない。
    #[test]
    fn rejects_unknown_permissions_fields_and_api() {
        let with = |from: &str, to: &str| Manifest::parse(&CALCULATOR.replace(from, to));
        assert!(with("[\"audio\"]", "[\"camera\"]").is_err());
        assert!(with("api = 1", "api = 99").is_err());
        assert!(with("permissions =", "unknown = 1\npermissions =").is_err());
    }
}
