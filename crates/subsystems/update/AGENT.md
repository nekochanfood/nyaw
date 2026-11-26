# crates/subsystems/update

層: 1 Subsystems

アプリの更新に必要な OS 側の処理を担当する。GitHub Releases のリリース一覧の取得とパース、インストーラーのダウンロードとサイズ・SHA-256 の検証、およびインストーラーの起動を行う。どのリリースを推奨するか、いつ確認するかは上位の [`crates/updater`](../../updater/AGENT.md) が決める。

通信（`ureq`）、ファイル、プロセスを扱う処理を本クレートに置くことで、`updater`（層 5）は OS の API を直接使わない。インストーラーの形式（Inno Setup）とファイル名の規則（`nyaw-<バージョン>-setup.exe`）を扱うコードは本クレートにしかないため、他のプラットフォームの配布方式に対応する場合も、変更は本クレートに閉じる。

## リリース一覧の取得とパース

- `check(url)` は GitHub の API からリリース一覧を取得し、`parse_releases` でパースする。下書きと、インストーラー・SHA-256（アセットの `digest`）・ビルド番号のいずれかが欠けたものは除外する。
- ビルド番号はリリースノート末尾の注釈 `<!-- nyaw-build: <番号> -->`（リリースワークフローが追加する）から読む（`notes_build`）。
- 一覧の URL はリポジトリ（`Cargo.toml` の `repository`）から作る（`releases_url`）。

## ダウンロードと検証

`download` はインストーラーを指定のディレクトリ（既定は `download_dir()`、一時ディレクトリの下）へ保存し、サイズと SHA-256 を照合する。不一致の場合はファイルを削除してエラーを返す。ファイル名がインストーラーの規則に合わないもの（パス区切りを含むものを含む）は取得しない。

| 対象 | 上限サイズ | タイムアウト時間 |
|---|---|---|
| リリース一覧取得 | 4 MiB | 30 秒 |
| インストーラーダウンロード | 512 MiB | 30 分 |

## インストーラーの起動

`run_installer` / `start_installer` はインストーラーをサイレント引数（`/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /WAITPID=<pid> /RELAUNCH=1 /ARGS=<再起動時の引数>`）で起動し、完了を待たずに戻る。インストーラーは nyaw のプロセスの終了を待ってからファイルを置き換え、再起動する。再起動時の引数は空白で結合されるため、空白・引用符を含むものは渡さない（`installer_args`）。

## 主な公開 API

```rust
Release; Installer;          // リリースと付属インストーラーの情報
check(url);                  // リリース一覧の取得（別スレッドで呼ぶ）
parse_releases(json);        // リリース一覧のパース
releases_url();              // 一覧を返す GitHub API の URL
download(release, dir, ..);  // ダウンロードとサイズ・SHA-256 の検証
download_dir();              // 既定のダウンロード先
run_installer(path);         // インストーラーの起動（現在と同じ引数で再起動させる）
start_installer(path, args); // インストーラーの起動（再起動時の引数を指定する）
fake;                        // ループバックの「GitHub」（test-support フィーチャー）
```

## テスト手法

- **単体テスト (`cargo test -p update`)**: ビルド番号の読み取り、リリースの除外条件、インストーラーの引数、およびループバックの「GitHub」（`fake`）を用いたダウンロードとサイズ・SHA-256 の検証を検査する。
