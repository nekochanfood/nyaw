# crates/subsystems/desktop

層: 1 Subsystems

ディスプレイ画面および個別アプリケーションウィンドウの列挙、キャプチャ画像の取得、および OS に対する直接的なタッチ入力の注入を担当する。Linux では OS キーボード（仮想キーボードから OS の入力先への送信）も担う。

Windows 環境では Windows Graphics Capture (WGC) および Synthetic Pointer API を使用する。Linux 環境の方式は「Linux の配信と操作」の節で述べる。Linux で扱うのはディスプレイ（画面）だけであり、個別のウィンドウとアプリの一覧・起動は Windows でのみ実装している。

## ウィンドウとアプリケーション情報の突合

ウィンドウの所属判定や後継ウィンドウの追跡など、OS 固有の知識（実行ファイル名や拡張子 `.exe` の扱い）を本サブシステムに集約する。

- `app_key(exe)`: 実行ファイル名を正規化して小文字かつ拡張子なしの検索キーを生成する。
- `InstalledApp::owns(&Source)`: 指定されたウィンドウが該当アプリケーションに属しているかを判定する。
- `Source::replacement` / `Source::successor`: キャプチャ対象のウィンドウが閉じられた際、タイトルやプロセス情報から後継ウィンドウを特定する。

## 画面・ウィンドウのキャプチャと配信

- **非同期ワーカーによる画像取得**: キャプチャ処理はバックグラウンドスレッドで実行され、メインスレッドは `poll()` を介して最新のフレーム画像のみを非同期に受け取る。OS 側の開始・停止・画像読み出しによってメインスレッドがブロックされることはない。
- **カーソル画像の分離取得**: キャプチャ映像内に OS カーソルを直接焼き込まず（WGC の `IsCursorCaptureEnabled` を無効化）、別スレッドが `GetCursorInfo` からカーソルのビットマップとホットスポット座標を独立して取得する。取得したカーソル画像は `Overlay::set_os_cursor` を通じて VR 空間内のポインタ表示へ反映される。
- **アスペクト比の追従**: パネルは元のキャプチャ解像度のアスペクト比を維持して表示される。ダッシュボードにドッキングしている間は縦方向の寸法が固定され、横幅が比率に応じて追従する。

## OS 入力注入とウィンドウフォーカス制御

- **タッチ接点としての入力注入**: 配信パネルに対するポインタ操作は GPUI のマウス変換を経由せず、左右の手をタッチ接点（ID 0 / 1）として OS 側へ直接注入する。手の追跡喪失やパネルの把持開始時にはタッチが自動キャンセルされる。
- **対象ウィンドウの前面化 (`FocusSource`)**: ウィンドウの配信開始時やダッシュボードでの切り替え時、対象ウィンドウを最小化から復帰させ、`SetForegroundWindow` を通じて OS の入力フォーカスを取得する。この処理も非同期ワーカー側で実行される。
- **未起動アプリの起動ハンドリング**: ランチャーから未起動のアプリを選択した場合、起動プロセスを要求すると同時に起動待機画面（`launch_spec`）を展開する。0.5 秒間隔で新規ウィンドウの出現を監視し、検出された段階で即座に配信へ移行する（20 秒間検出されない場合はタイムアウト表示）。

## Linux の配信と操作

表示サーバーに応じて、取り込みと入力の方式を次のように使い分ける（`src/linux/`）。環境変数 `NYAW_CAPTURE`（`x11` / `wlr` / `portal`）を指定すると、判定を省いてその方式を使う。

| 表示サーバー | 判定 | 画面の取り込み | ポインタの操作 | OS キーボード |
|---|---|---|---|---|
| X11 | `WAYLAND_DISPLAY` が無く `DISPLAY` がある | RandR のモニターを MIT-SHM（memfd の共有メモリ）で取り込む。XDamage で変化を待つ | XTest | XTest |
| wlroots 系（sway、Hyprland など） | `zwlr_screencopy_manager_v1` を提供する | wlr-screencopy の `copy_with_damage` で wl_shm へ写させる | wlr の仮想ポインタ（出力に結び付ける） | zwp_virtual_keyboard_v1 |
| その他の Wayland（KDE Plasma、GNOME） | 上のどちらでもない | xdg-desktop-portal の ScreenCast と PipeWire | portal の RemoteDesktop | portal の RemoteDesktop |

- **画像の受け渡し**: `Stream::start_with` にランタイムが取り込める形式（`vr::ImportFormats`）を渡すと、表示サーバーの画像を CPU で並べ替えずに `Frame::external`（`vr::ExternalImage`）として渡す。PipeWire ではランタイムの GPU が取り込める DRM 修飾子で DMA-BUF を交渉し、共有メモリ（XSHM、wl_shm、memfd）はページ境界に揃えて確保する。ランタイムが画像を手放すと、バッファは表示サーバーへ返る。取り込めない場合（novr など）は、従来どおり RGBA に並べ替えて渡す。ランタイム側の取り込みは [`../vr/AGENT.md`](../vr/AGENT.md) を参照する。
- **portal のセッションの共有**: portal はプロセスで 1 つのセッションを開き、全モニターと、ポインタ・キーボードの操作をまとめて許可させる。そのため、確認画面はモニターの数によらず 1 回だけ表示される。選択は `$XDG_DATA_HOME/nyaw/screencast-token` に保存し、次回からは確認画面を出さずに復元する。モニターと PipeWire のストリームは、Wayland の出力の論理座標で対応付ける。
- **許可待ちの表示（`Status::Permission`）**: portal の開始の要求が 0.4 秒以内に返らない場合、確認画面が表示されているとみなし、許可待ちを報告する。保存した選択を復元できる場合は数十 ms で返るため、この表示は出ない。同じセッションを待っている他の配信も許可待ちを報告する。開始の要求は確認画面の応答を待つため、許可されれば再試行の操作なしに配信が始まる。拒否された場合は `Reason::Denied` で失敗し、モニターが選ばれなかった場合は `Reason::NotShared` で失敗する。
- **ポインタの座標**: 0〜1 の座標は画素に丸めて送る（KWin は小数の座標を切り捨てるため、丸めずに送ると 1 画素手前に置かれる）。
- **カーソルと位置**: X11 は XFixes でカーソルの画像を、QueryPointer で位置を読む。portal はカーソルを映像に含めないメタデータモードを選び、PipeWire のメタデータから画像・ホットスポット・位置を読む。wlr-screencopy にはカーソルの画像を取得する手段がないため、取得せず、パネルには nyaw の通常のポインタを表示する（応急処置。ポインタの画像を無理に取得して置き換えることはしない）。
- **OS キーボード**: Linux で OS へキーを送るには表示サーバーとの接続（portal では画面の配信と同じセッション）が要る。そのため本サブシステムが `os::keyboard::Host` を実装し、カーネルは `desktop::os_keyboard()` で監視役を作る。配列の検出は `os::keyboard::detect` が行う。キーは evdev のキーコードで送り、文字列は次のように送る。
  - X11: 配列にある文字はそのキーで送り（必要なら Shift を付ける）、配列にない文字は空きのキーコードに割り当てて送る。割り当てはキーボードを閉じるまで残す。
  - wlroots: 文字ごとに 1 キーを割り当てた配列に差し替えて送り、送り終えたらコンポジタの配列に戻す。仮想キーボードは修飾キーを押しても修飾子が変わらないため、修飾子も送る。
  - portal: KWin の NotifyKeyboardKeysym は Shift などの段を付けずにキーを押す。そのため、コンポジタの配列（wl_keyboard.keymap を xkbcommon で解釈したもの）から文字のキーと段を引き、キーコードで送る。配列にない文字だけを keysym で送るが、コンポジタが配列から見つけられない文字は入力されない。

## 主な公開 API

```rust
Sources;                 // キャプチャ元（ディスプレイ・ウィンドウ）の一覧管理
Stream;                  // 個別キャプチャセッションの制御
Stream::start_with(..);  // ランタイムが取り込める形式を渡して開始する（Linux）
Stream::pointer();       // OS のポインタの位置（表示サーバーが知らせる場合）
Status::Permission;      // OS の確認画面で許可を待っている
Frame::external;         // ランタイムへ直接渡す表示サーバーの画像
Source;                  // キャプチャ元情報（名前、アプリアイコン、製品名等）
Cursor;                  // OS カーソル画像とホットスポット情報
capabilities();          // ホバーや最小化対応などのバックエンド能力問い合わせ
app_icon(aumid);         // AppUserModelId からのアプリアイコン非同期取得
os_keyboard();           // OS キーボードの監視役（Linux は表示サーバー経由で送信する）
```

## テスト手法

- **単体・モックテスト**: `desktop/test-support` フィーチャーを有効化し、モックを用いて最新画像の更新、タッチイベントの配送、およびセッション終了処理を検証する。
- **実機キャプチャテスト**: `crates/subsystems/desktop/tests/windows_capture.rs` および `testkit/tests/scenarios/desktop_stream.rs`（いずれも `#[ignore]` 指定）を用いて実デスクトップ環境での動作を検証する。
- **Linux の実機テスト**: `tests/linux_capture.rs`（`#[ignore]` 指定。`-- --ignored --test-threads=1` で実行する）は、X11 の項目を rootful の Xwayland を別に起動して検証する（取り込みの画素、ポインタの位置とクリック、カーソルの画像、OS キーボード）。portal の項目は実際のセッションで検証する（許可待ちから配信の開始まで、ポインタの位置、カーソル、テスト用の XWayland ウィンドウへの OS キーボード）。許可待ちの項目は保存した選択を消して確認画面を出させるため、許可の操作が要る。`NYAW_KEEP_CHOICE=1` を指定すると選択を残し、許可待ちの確認を省く。
- **wlroots のテスト**: wlroots 系のコンポジタが無い環境でも検証できるよう、`tests/wlr_protocol.rs` はテスト内に最小限のコンポジタ（wl_output、wl_shm、wl_seat、wlr-screencopy、wlr の仮想ポインタ、仮想キーボード）を立て、取り込み・ポインタ・キーボードの手順を検証する。
- **ランタイムまでの検証**: `NYAW_E2E_DESKTOP=1 cargo test -p testkit --test runtimes -- --ignored steamvr` は、このセッションの画面を SteamVR（null ドライバ）へ配信し、取り込んだ画像が届くことを読み戻して確かめる（[`../../testkit/AGENT.md`](../../testkit/AGENT.md)）。
