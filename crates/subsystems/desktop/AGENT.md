# crates/subsystems/desktop

層: 1 Subsystems

画面・ウィンドウの取得元の一覧・画像取得・OS タッチ注入（「画面・ウィンドウ配信」）

## 主な公開 API

`capabilities`（ホバー・最小化の対応）, `Sources`, `Stream`, `Source`（ウィンドウは `app` に実行ファイル名（Windows）、`icon` にアプリのアイコン。Windows はウィンドウのもの、無ければ実行ファイルのもの。画面の `name` はモニターの製品名（`QueryDisplayConfig`。同じ製品名が並べば番号を添える）、無ければ「Display 番号」）, `Icon`, `Status`, `Touch`

## 画面・ウィンドウ配信

- `crates/subsystems/desktop` が取得元の一覧、画像取得、OS タッチ注入を担う。Windows は WGC と Synthetic Pointer。上位へは `kernel::desktop` 経由で公開する。
- `Sources::refresh` と `Stream::start` はワーカーを起こすだけ。メインスレッドは `poll` で最新画像だけを受け取り、OS の開始・取得・停止・入力注入を待たない。入力キューが詰まったら接点を残さずセッションを止める。
- `PanelCommand::Stream(Some(source))` で既存パネルを配信先にし、`None` で通常の UI に戻す。`PanelStatus::desktop` が開始中・配信中・終了・エラーを持つ。画像はカーネルから直接 overlay へ渡し、UI の描画で上書きしない。横幅を保って配信元の縦横比に追従する（ダッシュボードにはめている間は縦を保つ）。
- 配信先のポインタは GPUI のマウス変換を通さず、左右を ID 0 / 1 のタッチ接点としてサブシステムへ送る。パネル外でも解放まで捕捉し、手の追跡喪失・掴み開始でキャンセルする。閉鎖・エラー・再接続では OS セッションを解放する。
- ウィンドウの配信を始めたとき・隠していた配信をバーのボタンで出したとき・ダッシュボードを出し直したとき（はまっている配信）は、そのウィンドウを OS で前に出してフォーカスする（OS のキーボード・マウスの操作が届くように。`PanelCommand::FocusSource` → `desktop::Stream::focus`。前面化はワーカーが行い、メインスレッドは待たない。Windows は最小化を戻して `SetForegroundWindow`（拒まれたら入力キューを一時的に共有して））。
- 映しているウィンドウが閉じたら（配信が終了）、配信のパネルも閉じる。失敗は最小化でも起きるので、取得元を取り直してウィンドウが無くなっていたときだけ閉じる（ダッシュボードの `Mirror`）。
- ランチャーで起動していないアプリを押すと、起動を頼むと同時にそのアプリの画面（`launch_spec`）がはまり、アイコン・名前・「起動しています」を出して待つ。画面自身が 0.5 秒ごとにウィンドウを探し、出たらそこで映し始める（20 秒出なければ見つからないと出す）。
- Testing の `desktop-test` が一覧更新・配信開始・停止と状態表示を持つ。配信パネルは独立して掴んで移動でき、ナビゲーションバー（タイトルは取得元の名前）で閉じると配信も止まる。バーの左端の `nav-keyboard`（`PanelSpec::with_os_keyboard`）で OS キーボード（OS の前面のアプリへ送る仮想キーボード）を出し入れし、出ている間は強調色になる。キーボードは頭の前の打ちやすい位置に出る。統計の HUD と仮想キーボード（自前の X）はバーを付けない。
- 下位の確認は `cargo test -p desktop -p kernel -p overlay -p subsystems --no-default-features --lib`。UI の単体テストは `cargo test -p testing -p ui -p i18n --lib`。`desktop/test-support` のモックは最新画像とタッチ配送、セッション停止を検査できる。
- 実デスクトップの E2E は `crates/subsystems/desktop/tests/windows_capture.rs`、UI を通すシナリオは `crates/testkit/tests/desktop_stream.rs`。すべて `#[ignore]`。実行には明示的な指定と、ネイティブのテストでは操作できるデスクトップが必要。`cargo check --tests` / Clippy は OS を操作せず型を検査できる。
- Windows のウィンドウ操作はデスクトップで対象が見えている場合に限り、別ウィンドウへの誤入力を防ぐ。
