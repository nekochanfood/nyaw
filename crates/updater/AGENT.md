# crates/updater

層: 5 画面（`nyapp` と同じ層の横。ダッシュボードと設定画面が使う）

アプリの更新。GitHub のリリースから今より新しいビルドを見つけて知らせ、**ユーザーが選んだときだけ**落として入れ替える（勝手には入れない）。

## 主な公開 API

`VERSION`（表示する版）, `BUILD`（ビルド番号）, `Release`, `Installer`, `releases_url`, `check(url)`, `parse_releases`, `newest(releases, build, channel)`, `latest`, `notes_build`, `is_installer`, `download`, `installer_args`, `run_installer`, GPUI のグローバル `Updater`（`check_now` / `install` / `available` / `state` / `reveal` / `take_reveal`）, `Step`

## 版とビルド番号

- 版は `v<CalVer>[-<名前>.<数>]`（`v2026.0.1`・`v2026.0.1-beta.1`・`v2026.0.1-patch.3`・`v2026.0.1-potato.1`）。名前は自由（インストーラーのファイル名に使えない文字と空白だけ除く）なので、**版の文字からは新旧を決めない**。
- 新旧は**ビルド番号**で比べる。ビルド番号 = HEAD までのコミット数（`git rev-list --count HEAD`）。main は直線の履歴（rebase でマージ）なので、後のコミットほど大きい。数えるだけでどこにも書かないので、上げ忘れが無い。
  - `build.rs` が埋め込み（`BUILD`）、リリースのワークフローも同じ数え方で、リリースノートの末尾に**見えない注釈** `<!-- nyaw-build: <番号> -->` として足す（`notes_build`。GitHub も nyaw の Markdown も HTML のコメントは描かない）。ファイル名（`nyaw-<版>-setup.exe`）にも画面にも出さない。アプリは API の 1 回の応答だけで全リリースを並べられる。注釈が消されたリリースは勧めない（安全側）。
  - git の無い所でのビルドは 0（どのリリースも新しく見える）。リリースのワークフローは履歴を全部取る（`fetch-depth: 0`）。
- 表示する版（`VERSION`）はリリースのワークフローがタグから渡す `NYAW_VERSION`（無ければ Cargo.toml の版）。見つけたリリースの版はタグから（GitHub はアセットの名前の記号を置き換えることがある）。ワークフローはタグの形と、全パッケージの版がタグの版かその CalVer の部分であることを確かめる。
- 新しいリリース = 下書きでなく、インストーラー（`is_installer`）とその SHA-256（`digest`。無ければ確かめられないので勧めない）とビルド番号の注釈があり、番号が今より大きく、チャンネルで受け取るもの。その中で一番大きいもの（`newest`）。

## チャンネル

設定の `update_channel`（`stable` / `nightly`。既定 `stable`。`kernel::UpdateChannel`、`Setting::UpdateChannel`）。設定のアップデートの「アップデートのチャンネル」（`setting-update-channel`）で選ぶ。

- **stable**: 正式なリリースだけ。GitHub でプレリリースの印が付いたもの（`Release::prerelease`）は勧めない。
- **nightly**: プレリリース（alpha・beta など）も勧める。
- リリースのワークフローは、名前の付かない版を正式なリリース、名前の付いた版（`-<名前>.<数>`）をプレリリースとして出す。手で出すときは GitHub の印で決まる。
- 確かめた一覧はチャンネルを問わず `Updater::found` に覚え、勧めるもの（`latest`）はそのときのチャンネルで選ぶ。切り替えるとすぐ変わる（確かめ直さない）。
- 今より古いビルドには戻さない（nightly で新しいビルドを入れてから stable に替えると、それより新しい正式なリリースが出るまで勧めない）。

## 流れ

1. **確かめる**（`check_now`。別スレッド）: GitHub の API `GET /repos/<owner>/<repo>/releases?per_page=30` を 1 回（リポジトリは Cargo.toml の `repository`）。ダッシュボードが起動時と 6 時間ごと（`RECHECK`。nyaw は常駐するので。リリースビルドだけで、開発中のビルドとテストは確かめない）と、設定の「今すぐ確認」（`update-check`）。
2. **知らせる**: 見つけたら（`latest`。チャンネルで選ぶ）。ダッシュボードのバーのボタン（`dashboard-update`。押すと設定をはめて `reveal`）と、設定のアップデート（`settings-update`。版・公開日・「アップデートして再起動」・リリースノート（`design::components::markdown`）・リリースのページ）。
3. **入れる**（`install`。ユーザーが押したときだけ）: インストーラーを `%TEMP%\nyaw-update` に落とし（進み具合は `Step::Downloading`）、大きさと GitHub が知らせる SHA-256（アセットの `digest`。必須）を確かめ、`/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /WAITPID=<pid> /RELAUNCH=1` で走らせて、アプリを終える（`PanelControl::quit`）。
4. **インストーラー**: ウィザードもメッセージも出さず、ユーザー単位（`PrivilegesRequired=lowest`）なので UAC も出ない。`WAITPID` のプロセスが終わるのを待って（60 秒まで）入れ替え、`RELAUNCH` で nyaw を起動し直す（引数なしで起動する）。

## 上限

| | |
|---|---|
| リリースの一覧 | 4 MiB・30 秒 |
| インストーラー | 512 MiB・30 分 |

## テスト

- `src/tests.rs`: リリースノートの注釈からビルド番号とインストーラーの名前、一覧の読み方と新旧（版の文字によらない・下書きと、インストーラーか番号の無いものは除く）、ループバックのサーバーから落として確かめる（SHA-256・大きさの違いは消してエラー）、確かめてから入れる流れ（GPUI のグローバル。インストーラーの走らせ方は差し替え）。
- E2E: `crates/testkit/tests/scenarios/update.rs`（設定の画面で、確かめる → 版とリリースノート → 押したときだけ落として走らせる。ダッシュボードのボタンからの `reveal`）。バーのボタンは `dashboard` の単体テスト。
- 実際のリリースでの確認は、GitHub に `v2026.0.1-patch.<n>` を出し、古い方を入れて新しい方に上げる（ビルド番号の違う 2 つのコミットから出す）。

## 未実装

- 自動で確かめない設定は無い。
- 起動し直すとき、元の引数（`--novr` など）は渡さない。
