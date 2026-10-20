# nyaw の変更

crates.io の ashpd 0.13.13（MIT）を元にしています。

`src/proxy.rs` で `org.freedesktop.portal.Request` のプロパティキャッシュと
`version` 取得を無効にしています。このインターフェースにはプロパティが無く、オブジェクト作成前に
Response の購読を始めるため、プロパティを取得すると UnknownMethod の警告になります。
通常の Portal インターフェースのプロパティとバージョン取得は維持します。

部分的なフィーチャー構成で使われない上流の共通ヘルパーについては、
`src/lib.rs` で `dead_code` の警告を許可しています。

回帰テスト:

```sh
dbus-run-session -- cargo test -p desktop --test portal_requests -- --ignored
```
