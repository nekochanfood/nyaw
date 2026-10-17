//! 画面の文言の多言語対応。対応言語は [`Lang`]（英語・日本語）。
//!
//! 文言は言語ごとの表 `locales/<code>.toml` に置き、キー（`testing.title` のような `.` 区切り。TOML の表がそのまま区切りになる）で引く。
//! 画面は `render` で [`t`] / [`tf`] を呼ぶ。言語は GPUI のグローバル [`Lang`] で、切り替えると全ウィンドウが描き直される
//! （`ui::sync_panel` がカーネルの設定から反映する）。表に無いキーは英語、英語にも無ければキーそのものを出す。
//!
//! キーは `i18n::t(cx, "…")` / `i18n::tf(cx, "…", …)` の第 2 引数に文字列で直に書く。テストがソースを走査して、
//! 使っているキーがどの言語の表にもあるかを確かめる。

use std::collections::{BTreeMap, HashMap};
use std::fmt::Display;
use std::sync::OnceLock;

use gpui::{App, SharedString};

/// 画面の言語。`kernel::Language` に対応する。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    #[default]
    En,
    Ja,
}

impl gpui::Global for Lang {}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Ja];

    /// 表のファイル名・設定の値（`en` / `ja`）。
    pub const fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ja => "ja",
        }
    }

    /// その言語自身での名前（切り替えのボタンに出す。どの言語で表示していても読めるように）。
    pub const fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Ja => "日本語",
        }
    }

    /// 次の言語（切り替えのボタン用）。
    pub fn next(self) -> Lang {
        let i = Lang::ALL.iter().position(|&l| l == self).unwrap_or(0);
        Lang::ALL[(i + 1) % Lang::ALL.len()]
    }

    /// 今の言語（グローバルが無ければ英語）。
    pub fn current(cx: &App) -> Lang {
        cx.try_global::<Lang>().copied().unwrap_or_default()
    }

    fn source(self) -> &'static str {
        match self {
            Lang::En => include_str!("../locales/en.toml"),
            Lang::Ja => include_str!("../locales/ja.toml"),
        }
    }
}

type Table = HashMap<String, &'static str>;

/// 言語の表（キー → 文言）。初めて引いたときに埋め込みの TOML を読む。
fn table(lang: Lang) -> &'static Table {
    static TABLES: OnceLock<[Table; 2]> = OnceLock::new();
    let tables = TABLES.get_or_init(|| Lang::ALL.map(parse));
    &tables[Lang::ALL.iter().position(|&l| l == lang).unwrap()]
}

/// TOML の入れ子の表を `.` 区切りのキーに平らにする。文言は解放しない（プロセスの間ずっと使う）。
fn parse(lang: Lang) -> Table {
    let root: toml::Table = lang
        .source()
        .parse()
        .unwrap_or_else(|e| panic!("locales/{}.toml is not valid TOML: {e}", lang.code()));
    let mut out = Table::new();
    fn walk(prefix: &str, t: &toml::Table, out: &mut Table) {
        for (k, v) in t {
            let key = if prefix.is_empty() {
                k.clone()
            } else {
                format!("{prefix}.{k}")
            };
            match v {
                toml::Value::Table(t) => walk(&key, t, out),
                toml::Value::String(s) => {
                    out.insert(key, Box::leak(s.clone().into_boxed_str()));
                }
                other => panic!("{key}: a message must be a string, got {other}"),
            }
        }
    }
    walk("", &root, &mut out);
    out
}

/// `lang` での `key` の文言。表に無ければ `None`。
pub fn get(lang: Lang, key: &str) -> Option<&'static str> {
    table(lang).get(key).copied()
}

/// `lang` で `key` を引く。無ければ英語、それも無ければキーそのもの。
pub fn translate(lang: Lang, key: &str) -> SharedString {
    match get(lang, key).or_else(|| get(Lang::En, key)) {
        Some(s) => SharedString::new_static(s),
        None => SharedString::from(key.to_owned()),
    }
}

/// 今の言語で `key` を引く。
pub fn t(cx: &App, key: &str) -> SharedString {
    translate(Lang::current(cx), key)
}

/// 今の言語で `key` を引き、`{name}` を `args` の値で置き換える。
pub fn tf(cx: &App, key: &str, args: &[(&str, &dyn Display)]) -> SharedString {
    fill(&t(cx, key), args).into()
}

fn fill(template: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut s = template.to_owned();
    for (name, value) in args {
        s = s.replace(&format!("{{{name}}}"), &value.to_string());
    }
    s
}

/// どれかの言語の表にあるキー（並べ替え済み）。ローカライズの確認画面が一覧に使う。
pub fn keys() -> Vec<&'static str> {
    let mut all = BTreeMap::new();
    for lang in Lang::ALL {
        for k in table(lang).keys() {
            all.insert(k.as_str(), ());
        }
    }
    all.into_keys().collect()
}

/// `lang` の表に無いキー（ほかの言語にはあるもの）。
pub fn missing(lang: Lang) -> Vec<&'static str> {
    keys()
        .into_iter()
        .filter(|k| get(lang, k).is_none())
        .collect()
}

/// 文言の中の `{name}` の名前（並べ替え済み）。
#[cfg(test)]
fn placeholders(s: &str) -> Vec<&str> {
    let mut v: Vec<&str> = s
        .split('{')
        .skip(1)
        .filter_map(|p| p.split_once('}').map(|(name, _)| name))
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_has_every_key() {
        for lang in Lang::ALL {
            assert_eq!(missing(lang), Vec::<&str>::new(), "{}", lang.code());
        }
    }

    #[test]
    fn placeholders_match_across_languages() {
        for key in keys() {
            let en = placeholders(get(Lang::En, key).unwrap());
            for lang in Lang::ALL {
                assert_eq!(
                    placeholders(get(lang, key).unwrap()),
                    en,
                    "{key}: {{…}} differ in {}",
                    lang.code()
                );
            }
        }
    }

    #[test]
    fn falls_back_to_english_then_to_the_key() {
        assert_eq!(translate(Lang::Ja, "common.close"), "閉じる");
        assert_eq!(translate(Lang::En, "common.close"), "Close");
        assert_eq!(translate(Lang::Ja, "no.such.key"), "no.such.key");
    }

    #[test]
    fn fill_replaces_named_placeholders() {
        assert_eq!(
            fill("{n} of {max} ({n})", &[("n", &3), ("max", &"10")]),
            "3 of 10 (3)"
        );
    }

    #[gpui::test]
    fn follows_the_global_language(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            assert_eq!(t(cx, "common.close"), "Close", "English until set");
            cx.set_global(Lang::Ja);
            assert_eq!(t(cx, "common.close"), "閉じる");
            assert_eq!(tf(cx, "testing.clicked", &[("count", &2)]), "クリック 2 回");
        });
    }

    /// 画面のソースで使っているキー（`i18n::t(cx, "…")` / `i18n::tf(cx, "…", …)`）が、どの言語の表にもある。
    #[test]
    fn keys_used_in_the_sources_exist() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut used = Vec::new();
        let mut stack = vec![crates];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // この crate のテストは見本のキーを書いているので除く。
                    if !path.ends_with("target") && !path.ends_with("i18n") {
                        stack.push(path);
                    }
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let src = std::fs::read_to_string(&path).unwrap();
                    for key in used_keys(&src) {
                        used.push((path.clone(), key));
                    }
                }
            }
        }
        assert!(!used.is_empty(), "no i18n::t calls found");
        for (path, key) in used {
            for lang in Lang::ALL {
                assert!(
                    get(lang, &key).is_some(),
                    "{}: {key} is not in locales/{}.toml",
                    path.display(),
                    lang.code()
                );
            }
        }
    }

    /// `i18n::t(` / `i18n::tf(` の直後の最初の文字列リテラル（第 2 引数のキー）。
    fn used_keys(src: &str) -> Vec<String> {
        let mut keys = Vec::new();
        for call in ["i18n::t(", "i18n::tf("] {
            for (at, _) in src.match_indices(call) {
                let rest = &src[at + call.len()..];
                let Some((_, key)) = rest.split_once(',') else {
                    continue;
                };
                let Some(key) = key.trim_start().strip_prefix('"') else {
                    continue;
                };
                if let Some(close) = key.find('"') {
                    keys.push(key[..close].to_owned());
                }
            }
        }
        keys
    }

    #[test]
    fn used_keys_reads_the_literal_after_cx() {
        let src =
            r#"let a = i18n::t(cx, "a.b"); let b = i18n::tf(window_cx, "c.d", &[("n", &1)]); let c = i18n::tf(cx, key, &[("pct", &1)]);"#;
        assert_eq!(used_keys(src), ["a.b", "c.d"]);
    }
}
