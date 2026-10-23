# crates/i18n

層: 4 横

画面の文言の多言語対応（英語・日本語）。言語ごとの表 `locales/{en,ja}.toml`（キーは `.` 区切り、差し込みは `{name}`）を埋め込み、GPUI のグローバル `Lang`（`ui::sync_panel` がカーネルの `language` から反映）で引く。無いキーは英語、それも無ければキー。テストが、全言語に全キーと同じ差し込みがあるか、ソースの `i18n::t(cx, "…")` のキーが表にあるかを確かめる

## 主な公開 API

`t(cx, key)`, `tf(cx, key, &[(name, &value)])`, `Lang::{En, Ja, ALL, native_name, current}`, `get`, `keys`, `missing`
