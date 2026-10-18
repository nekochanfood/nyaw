# AGENT.md — ニャプリ（nyapp）

nyaw の上で動くアプリを **ニャプリ（nyapp）** と呼ぶ。このクレート（`crates/nyapp`）は、ニャプリを入れる・消す・更新する・
動かす・描く基盤で、Android でいう PackageManager・ActivityManager・アプリのサンドボックスの役を 1 つにまとめたもの。
ニャプリを書く側の SDK は `crates/nyapp-sdk`、試作のニャプリは `nyapps/`（別のワークスペース）にある。

## 構想

着想は Linux と Android。カーネル・UI・アプリの 3 層に分ける。

```
┌ ニャプリ ────────────────────────────────────────────────┐
│  システム: wrist・ダッシュボード・ランチャー・デバッグ・設定      │  ← nyaw に組み込み（ネイティブ・特権）
│  ユーザー: 電卓・時計・目玉・サンドボックス診断 …              │  ← .nyapp（wasm・サンドボックス）
├ UI（crates/ui・design）─────────────────────────────────┤  ← ニャプリとカーネルの両方が頼るもの
├ カーネル（crates/kernel・subsystems）────────────────────┤  ← プラットフォームごとにビルド
└──────────────────────────────────────────────────────┘
```

- **ニャプリはプラットフォームに依らない 1 つのバイナリ**（jar・apk と同じ）。`wasm32-unknown-unknown` の wasm を
  `.nyapp` に入れて配り、nyaw の中の VM（wasmi）で動かす。Windows でも Linux でも同じ `.nyapp` が動く。
- **カーネルはプラットフォームごとにビルドする**。ニャプリはカーネルに直接触れず、このクレートのホスト関数だけを通る。
- **UI は「何を出すか」だけをニャプリが宣言する**。ニャプリは画面の木（`nyapp_sdk::Node`。行・列・カード・格子・文字・
  ボタン・スイッチ・図形の面）を返し、見た目（色・寸法・ホバー・押下・波紋・テーマ）はホストが `design` の部品で決める。
  どのニャプリも nyaw の見た目になり、テーマや言語を変えても崩れない。
- **システム UI もニャプリ**（Android の SystemUI と同じ思想）。wrist・ダッシュボード・ランチャー・デバッグパネル・設定画面は
  ほかのニャプリと同じく逆ドメインの ID（`net.bettaworx.*`）を持ち、起動中の表（`Running`）に載る。ただし本体に組み込まれ
  （GPUI で直接描くネイティブのもの）、サンドボックスの外でカーネルの状態に触れ、消せず、同じ ID のパッケージは入れられない
  （`system.rs`）。毎フレーム頭や手に追従し、レーザーやキーボードを扱うものは、VM を挟むと遅れるので特権側に置く。

## 思想

1. **ニャプリが落ちても nyaw は落ちない。** trap・燃料切れ（無限ループ）・メモリの上限・呼び出しの深さ・壊れた画面は、
   どれもエラーとしてホストに返る。落ちたニャプリのパネルは閉じ、視界の中央に「強制終了しました」のダイアログ
   （再起動・閉じる）を出す（`view.rs` の `CrashDialog`）。ほかのパネルとニャプリはそのまま動き続ける。
2. **1 ニャプリ 1 サンドボックス（マルチテナント）。** 開いたパネルごとに専用の Engine・Store・線形メモリ・燃料・保存領域を
   持ち、ニャプリどうしで共有するものは無い。ニャプリに見えるのはホスト関数（import モジュール `nyaw`）だけで、ファイル・
   ネットワーク・環境変数・ほかのアプリには届かない（WASI も渡さない）。
3. **外から来たものは信じない。** wasm は検証してから動かす（wasmi の `validate`）。ニャプリから受け取る位置と長さは範囲を
   確かめ、外れていれば書かずに `INVALID` を返す。zip の展開・画面の JSON・ログ・保存領域・更新のダウンロードには上限がある。
   更新は https（手元の確認用にループバックだけ http）で、SHA-256・ID・版がフィードのとおりのときだけ入れ替える。
4. **メインスレッドを止めない**（nyaw 全体の方針）。パッケージの読み込み・wasm の検証と初期化・アイコンの展開・一覧の読み直し・
   保存領域の書き込み・更新はすべて別スレッド。メインスレッドで呼ぶのは出来事を渡して画面を受け取る 1 回だけで、その燃料は
   `CALL_FUEL` で抑える。
5. **権限はマニフェストに書いたものだけ。** 書いていないものは `DENIED`。知らない権限を書いたマニフェストは読まない。
   今は入れた時点で認める（Android の昔のインストール時の権限と同じ）。
6. **足すだけ、消さない。** 画面の語彙・出来事・ホスト関数・マニフェストの項目は足すだけにして、意味を変えたり消したりしない
   （入っているニャプリが壊れる）。壊す変更が要るときは `nyapp_sdk::API` の版を上げる。

## 形式

### ID とファイル名

逆ドメイン表記（`net.bettaworx.calculator`）。`.` で区切った 2 つ以上の部分で、どれも英小文字で始まり英小文字・数字・`_` だけ。
パッケージのファイル名は `<id>.nyapp`。名前が ID と違うパッケージは一覧に出さない。

### `.nyapp`（zip）

| 中身 | |
|---|---|
| `manifest.toml` | マニフェスト（下） |
| `app.wasm` | `wasm32-unknown-unknown` の cdylib |
| `icon.svg` / `icon.png` | 任意。アイコン（Android のアプリアイコンと同じく、パッケージが持つ）。ベクター（SVG。256px で描く）とラスター（PNG。正方形で 192px 程度）のどちらでもよく、両方あれば SVG。無い・読めないときは nyaw がアクセントの色の角丸の四角に名前の頭文字を描く（`icon.rs`） |

### マニフェスト

```toml
id = "net.bettaworx.clock"
name = "Clock"
version = "2026.0.1"            # `.` / `-` で区切った部分を数として比べる（更新の判定）
api = 1                         # nyapp_sdk::API
permissions = ["audio"]         # audio（音を鳴らす）/ media（再生中のタイトルを読む）

[names]                         # 言語ごとの名前（無ければ name）
ja = "時計"

[update]                        # 任意。更新情報（AppImage の Update Information に当たる）
url = "https://example.com/nyapps/net.bettaworx.clock/latest.toml"
```

### 更新（`update.rs`）

`[update].url` は最新版を知らせるフィード（TOML）を指す。ランチャーの「更新を確認」で、更新情報を持つニャプリごとに読み、
新しければパッケージを落として確かめ、入れ替える（次に開いたときから新しい版）。

```toml
id = "net.bettaworx.clock"
version = "2026.1.0"
package = "net.bettaworx.clock-2026.1.0.nyapp"   # 相対ならフィードの場所から
sha256 = "…"                                     # 必須
```

### ABI（`nyapp_sdk::API` = 1）

ニャプリが書き出す: `memory`、`nyapp_alloc(len) -> ptr`、`nyapp_init()`、`nyapp_view() -> (ptr << 32 | len)`（画面の木の JSON）、
`nyapp_event(ptr, len)`（出来事の JSON。`Click` / `Toggle` / `Pointer` / `Tick`）。SDK の `export!` が作る。

ホストが用意する（import モジュール `nyaw`）: `log`、`now_ms`、`utc_offset`、`random`（暗号用ではない）、`language`、
`set_ticker`（周期の `Tick`。16ms 未満は延ばす）、`has_permission`、`storage_get` / `storage_set` / `storage_remove`
（ニャプリごとの保存領域。キーと値の合計 1 MiB まで）、`beep`（権限 `audio`）、`now_playing`（権限 `media`）。
`set_activity` / `clear_activity`（進行中の作業の知らせ。下）。
文字列を返すものは「バッファに書けるだけ書き、本当の長さを返す」形。失敗は負の値（`nyapp_sdk::code`）。

### 進行中の作業の知らせ（ライブアクティビティ。`activity.rs`）

タイマーのように、ニャプリを閉じても進み続けるものを、ダッシュボードのバーのクイック設定と再生中の間に出す
（iPhone の Dynamic Island のような進み具合の表示。`dashboard-activity-<id>`。アイコン・残り時間などの文字・進み具合のバー）。
押すとそのニャプリがはまる。ニャプリごとに 1 つ。

- サンドボックスはパネルを閉じると消えるので、知らせは nyaw（GPUI のグローバル `activity::Activities`）が持ち、閉じても残る。
- だからニャプリは残り時間ではなく、nyaw が数え続けられる形で渡す（`nyapp_sdk::Activity`）:
  `Countdown { ends_at_ms, total_ms }`（終わる時刻）/ `Stopwatch { started_at_ms }`（始めた時刻）/
  `Progress { text, progress }`（時刻で進まないもの。止めたタイマーなど）。
- カウントダウンが終わると「終わりました」の状態で残る（押すと片付く）。ニャプリを閉じていれば、権限 `audio` があるとき
  手首（wrist）から知らせの音を鳴らす（開いていればニャプリ自身が知らせる）。
- ニャプリを開き直したら、保存領域から状態を読んで知らせを出し直す・消す（時計の試作がそうしている）。消したニャプリの知らせは消える。

Component Model（WIT）ではなく、素の wasm と JSON にしている。ツールチェーンに標準の `wasm32-unknown-unknown` だけで済み、
WASI を渡さないのでサンドボックスが最初から閉じていて、画面の木を再帰のまま受け渡せる。言語をまたいで型付きの約束が要るように
なったら、WIT に移す。

### 上限

| | |
|---|---|
| メモリ | 64 MiB（`MEMORY_LIMIT`） |
| 燃料（命令数） | 起動 5 億、1 回の呼び出し 2000 万（`CALL_FUEL`） |
| 保存領域 | 1 MiB（`STORAGE_QUOTA`） |
| 画面の JSON | 4 MiB |
| パッケージの中身 | マニフェスト 64 KiB・wasm 32 MiB・アイコン 1 MiB |
| 更新 | フィード 64 KiB・パッケージ 64 MiB・30 秒 |

## 置き場所

| | |
|---|---|
| パッケージ | `os::data_dir()/nyapps/<id>.nyapp`（Windows `%LOCALAPPDATA%\nyaw\nyapps`、Linux `$XDG_DATA_HOME/nyaw/nyapps`）。置けば入り、消せば消える |
| 保存領域 | `os::data_dir()/appdata/<id>/storage.json` |

## クレートの中

| ファイル | 責務 |
|---|---|
| `manifest.rs` | マニフェストの読み込みと検証（ID・ABI・権限・更新情報） |
| `package.rs` | `.nyapp` の読み書き（上限付きの展開） |
| `installed.rs` | 入っているものの一覧・入れる・消す |
| `sandbox.rs` | wasmi のサンドボックスとホスト関数・保存領域 |
| `view.rs` | 画面の木を `design` で描くパネル（`NyappView`）・強制終了のダイアログ（`CrashDialog`）・知らせの音 |
| `icon.rs` | アイコン（SVG・PNG）と代わりのアイコン |
| `update.rs` | 更新情報から最新版を確かめて入れ替える |
| `activity.rs` | 進行中の作業の知らせ（閉じても残る。カウントダウンの終わりを知らせる） |
| `system.rs` | システムのニャプリの ID |
| `lib.rs` | `Error`・起動中の表 `Running`（`opened` / `panel_of`） |
| `bin/nyapp.rs` | 開発用の道具 `cargo nyapps`（下） |

依存は `kernel`・`ui`・`design`・`i18n` の上（画面と同じ層の横）。ダッシュボード（ランチャー）・wrist・Testing がこのクレートに
依存する。ファイルの読み書き（パッケージ・保存領域）はこのクレート自身の責務で、OS の差（保存先）は `os::data_dir` が吸収する。

## ニャプリを作る

```sh
rustup target add wasm32-unknown-unknown
cargo nyapps build     # nyapps/ をビルドして target/nyapps/<id>.nyapp にまとめる
cargo nyapps install   # まとめて nyaw のパッケージのフォルダに入れる（ランチャーの「ニャプリ」に出る）
cargo nyapps pack <フォルダ> <wasm> [出力先]
```

新しいニャプリ = `nyapps/<名前>/` に `Cargo.toml`（`crate-type = ["cdylib"]`、`nyapp-sdk` に依存）・`manifest.toml`・
任意の `icon.svg` / `icon.png`・`src/lib.rs`（`App` を実装して `export!`）を置き、`nyapps/Cargo.toml` の `members` に足す。

### 試作

| ID | | 確かめること |
|---|---|---|
| `net.bettaworx.calculator` | 電卓（押した順に計算） | ふつうに使えるアプリ・格子・SVG のアイコン |
| `net.bettaworx.clock` | 時計・アラーム・タイマー・ストップウォッチ | 周期（`set_ticker`）・保存領域（アラーム・タイマー・ストップウォッチ。開き直すと続きから）・進行中の作業の知らせ（タイマーはカウントダウン、止めると残り、ストップウォッチ）・権限 `audio`（鳴らす）・言語。アラームはニャプリを開いている間だけ鳴る |
| `net.bettaworx.eyes` | 目玉（xeyes） | 図形の面とポインタ・PNG のアイコン |
| `net.bettaworx.sandbox` | サンドボックス診断 | ホスト関数を全部確かめて一覧にする（権限の有無・保存領域の上限・メモリ）。意図的なクラッシュ（panic・無限ループ・512 MiB・スタックあふれ）。部品の見本。アイコンを持たない（代わりのアイコンの確認） |

## UI への入口

- ダッシュボードのバーの、クイック設定と再生中の間: 進行中の作業の知らせ（上）。
- ランチャー（ダッシュボードの `Page::Launcher`）の「ニャプリ」: 入っているものをタイルで並べ（検索も効く）、押すとランチャーと
  入れ替わりにはまる（もう開いていればそのパネルをはめる）。見出しに「更新を確認」（`dashboard-nyapps-update`）と、
  消す状態の切り替え（`dashboard-nyapps-remove`。消す状態ではタイルがごみ箱になり、押すと消す）。
- Testing の `nyapp-test`: システムのものと入っているものの一覧と、選んだものの詳細（ID・種類・版・ABI・権限・更新情報・
  アイコン・パッケージ・起動中のパネル）。

## テスト

- 単体: `cargo test -p nyapp -p nyapp-sdk`（マニフェスト・パッケージ・一覧・サンドボックス（WAT で書いた最小のニャプリ。
  テストでだけ wasmi の `wat`）・クラッシュでパネルを閉じてダイアログを出す・ダイアログの再起動と閉じる・アイコン・更新
  （ループバックの HTTP サーバーから落として入れ替える・改ざんを拒む））。ニャプリ自身のロジックは `cd nyapps && cargo test`。
- 本物の wasm: `cargo test -p nyapp --test nyapps -- --ignored`（`nyapps/` をビルドして載せる。電卓の計算・サンドボックス診断が
  全部通る・意図的なクラッシュ 4 種がエラーで返りホストは動く・ニャプリどうしが見えない・周期・ポインタ・アイコン）。
- UI: `cargo test -p dashboard --lib launcher_lists_nyapps activities_sit`、`cargo test -p testing --lib nyapps`。

## 気をつけること

- wasmi は `portable-dispatch` にしている。`auto-dispatch` の末尾呼び出しは最適化の無いビルドでは積み上がり、止まらない
  ニャプリでホストのスタックがあふれた。
- ホスト関数からニャプリのメモリを引くのに `Caller::get_export` を使わない。インスタンスを生のポインタで引くので、Store を
  `Sandbox` に移した後に呼ぶと壊れたポインタを読む（Windows でアクセス違反になった）。インスタンスを作ったときの `Memory` の
  ハンドルを `Host` に持たせて使う。

## まだ無いもの

- バックグラウンドで動くニャプリ（アラームは開いている間だけ。知らせのカウントダウンは nyaw が数えるので閉じても終わりを知らせる）・通知
- 権限を使う時点で尋ねること（今はマニフェストに書けば認める）
- パッケージの署名（今は更新のときの SHA-256 だけ。同じ ID の別の作者のパッケージを入れられてしまう）
- ニャプリごとのスレッド（今はメインスレッドで呼び、燃料で抑える）
- 文字入力の部品（テキスト欄）・画像・スクロールの位置の指定
- システムのニャプリを wasm にすること（今は本体に組み込みのネイティブ）
