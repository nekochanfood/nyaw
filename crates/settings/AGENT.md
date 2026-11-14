# crates/settings

層: 5 画面

設定画面。ダッシュボードの設定ボタン（`menu-settings`。main が `settings::spec` を渡す）と Testing の `settings-test` から開く。GNOME の設定のようなツートーン: 左のサイドバー（地は `sidebar`）のカテゴリ（`settings-general` / `settings-appearance` / `settings-input` / `settings-display` / `settings-accessibility`。選んでいるものだけ文字が濃く、左端のアクセント色の印が前の行で縮み、選んだ行で伸びて出る）を押すと、右（地は `background`）の中身が切り替わる。閉じるのはナビゲーションバー。

## 主な公開 API

`Settings`（画面。`category: Category` / `pages: Vec<Page>` / `push(page)`）, `Category`（サイドバーのカテゴリ。`Category::ALL` の順に並ぶ）, `Page`（カテゴリから開く詳しい画面）, `spec(from, opened)`, `SIZE` / `PX`, `heading`（見出しの行。`testing::NavTest` も使う）, `SIDEBAR_BG` / `SIDEBAR_INDICATOR` / `SIDEBAR_INDICATOR_INSET`（E2E が色を見る）

## ファイルの分け方（ルール）

**カテゴリ・詳しい画面ごとに 1 ファイル**。カテゴリや画面を足すときは、ファイルを足して `lib.rs` の `Category` / `Page` と振り分け（`category_page` / `sub_page`）に 1 行ずつ足す。中身を `lib.rs` に書かない（同じ所を皆が触ってコンフリクトするので）。

| ファイル | 中身 |
|---|---|
| `lib.rs` | 骨組みだけ: サイドバー・積み重ね・カテゴリの入れ替え、どのカテゴリでも使う部品（選択の行 `select`・詳しい画面を開く行 `page_row(s)`・`heading`・`change`） |
| `general.rs` | 一般 = 言語・SteamVR と一緒に起動（`setting-auto-launch`。`Settings::auto_launch` が `Some` のとき = OpenVR のときだけ出る）、行 `settings-update` / `settings-about` |
| `appearance.rs` | 外観 = テーマの見本 `theme-preview`・テーマ `setting-theme`・パーソナルカラーの履歴（見本 `accent-<名前>` と、色を作るダイアログを開く `accent-custom`）、行 `settings-watch`。時計（`watch`）= 種類の絞り込み `watch-kind`・弧に並ぶ文字盤の見本 `watch-faces`（`watch-prev` / `watch-next` でも回せる）・`watch-set` |
| `input.rs` | 入力 = 利き手 `setting-dominant-hand`・ゲーム入力を止める `setting-block-input`（`switch_item`。行のどこを押しても `Setting::BlockInput` を切り替える）、行 `settings-pointer` / `settings-keyboard` |
| `display.rs` | 表示 = 見出し付きのグループでダッシュボード（距離 `setting-dashboard-distance` と高さ `setting-dashboard-height`（頭の高さからの相対）のスライダーの行（`-slider`）。この画面もダッシュボードに付いて動くので、動かしている間は値だけを見せ、離したときに決める（ダッシュボードはその場で置き直る））・グラフィックス（距離 LOD `setting-lod-quality`）・ミラーリング（FPS `setting-mirror-fps`） |
| `pointer.rs` | ポインタ = カーソルとビームの設定: 調整のしかた `setting-pointer-tuning`（`setting-tuning-simple` / `-detailed`）で、シンプルは 1 本のスライダー `setting-steadiness`（真ん中が既定。左半分で手ブレ抑えを弱め、右半分で抑えを緩める角度を広げる）、詳細は `setting-smoothing` / `setting-release` / `setting-laser-length`（線が消えていく長さ）。その下にカーソルの大きさ `setting-cursor-size`（スライダー `setting-cursor-size-slider`。50〜200%、10% の刻み）・ツールチップ `setting-tooltips`、音のタップ音 `setting-tap-sound`・ホバー音 `setting-hover-sound`（スイッチ。既定はオン） |
| `keyboard.rs` | キーボード = 上にキーボードの見本（`ui::prefs::keyboard_preview`。召喚したキーボードと同じ地 `keyboard-preview-card` に載り、どの種類も中身の幅と箱に収まる縮尺になる。押せて音も鳴るが、どこにも打たないダミー。`Settings::keyboard_preview` が持つ）、下に種類 `setting-keyboard`・キーの入力音 `setting-key-sound`（スイッチ。既定はオン）・その音量 `setting-key-volume`（スライダー `setting-key-volume-slider`。10% の刻み。入力音がオンの間だけ出る）で、どれも変えるとすぐ見本に出る |
| `update.rs` | アップデート = 今の版（`update-current`。ビルド番号は画面に出さない）・確認の状態（`update-status`）と「今すぐ確認」（`update-check`）、チャンネル（`setting-update-channel`。安定版 / ナイトリー。`../updater/AGENT.md` の「チャンネル」）、新しいリリースがあればその版（`update-version`）・「アップデートして再起動」（`update-install`。落としている間は `update-progress`、走らせたら `update-installing`）・リリースノート（`update-notes`。Markdown）・リリースのページ（`update-page`）。ダッシュボードのアップデートのボタンから開くと一般の上にこの画面を積んだ状態になる |
| `about.rs` | 情報 = ロゴ・バージョン、`setting-licenses` で情報の上に依存のライセンスの一覧を積む（`license-<名前>-<バージョン>`。押すとリポジトリをブラウザで開く（確かめてから。`ui::link`））、`setting-source` でこのリポジトリをブラウザで開く。一覧は `build.rs` が `cargo metadata` から nyaw の通常・ビルドの依存だけを集めてビルド時に埋め込む |

アクセシビリティは `ui::prefs::accessibility` 1 つなので `lib.rs` の振り分けに直接書く。

## 設計

- **段階的開示**: カテゴリにはよく変える設定だけを直接並べ、まとまった設定は詳しい画面（`Page`。`Settings::pages` に積む）に分け、カテゴリの中の `>` の付いた行から開く（右から入る。見出しの左の `settings-back` かスワイプバックで戻り、サイドバーでカテゴリを押すと、選んでいるものでも全部下ろす）。1〜2 行しかない設定は詳しい画面に分けない（押す回数を増やすだけなので、グループの見出しでまとめる）。
- 複数から選ぶ設定は選択の行（`setting-language` / `setting-theme` / `setting-dominant-hand` / `setting-mirror-fps` / `setting-keyboard` など）で、押すとポップオーバーに選択肢（`setting-lang-*` / `setting-theme-*` / `setting-hand-*` / `setting-fps-*` / `setting-keyboard-*`）が開き、外を押すと閉じる。選択肢とスライダーはすぐ全体の設定（`Setting`）に反映する。
- 「外観」「時計」「アクセシビリティ」は、セットアップウィザードと同じ部品（`ui::prefs::appearance` / `ui::prefs::watch_faces` / `ui::prefs::accessibility`。`../ui/AGENT.md` の「設定の部品」）。

## テスト

- 単体（`cargo test -p settings`）: ポインタの手ブレ抑えのスライダーの換算とカードの幅（`pointer.rs`）、キーボードの見本がどの種類でも収まること（`keyboard.rs`）。
- E2E は testkit の `scenarios/{navigation,prefs,pointer_settings,update}.rs`、見た目は `ui/tests/gpu/{look,a11y}.rs`。
