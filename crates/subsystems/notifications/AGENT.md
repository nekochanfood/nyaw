# crates/subsystems/notifications

層: 1 Subsystems

OS の通知（トースト）を読み、押されたら OS と同じ経路でアプリへ届ける。ワーカーが 1 秒ごとに通知センターを読み直し、起動してから届いた通知だけを知らせる（起動時にあったものは知らせない。通知センターから消えたら落とす）。アイコンの読み込み・押したときの呼び出しもワーカーで行い、メインスレッドは最新の一覧を受け取るだけ

## 主な公開 API

`Notifications::{start, fixed, now, activate, send}`（`fixed` は OS を見ずに決まった一覧を返し、届いた依頼（`Request::Activate` / `Request::Send`）を受け口に送る。E2E・見本用。`Kernel::set_notifications` で差す。`send` は `Outgoing`（見出し・本文・押すと開く URL・ボタン）を PowerShell の名前で Windows に出す。デバッグのテスト画面用）, `Notification`（`id` / `app` / `title` / `body` / `arrived` / `url` / `actions`）, `App`, `Action`, `Image`（= `desktop::Icon`）

## Windows

- **一覧**: `UserNotificationListener`（パッケージの ID が無いアプリでも読める。ユーザーが設定の「通知へのアクセス」を切っていれば何も届かない）。変化の見張り（`NotificationChanged`）はパッケージの ID が要るので使わず、読み直す。見出しは通知の 1 つ目の文字、本文は残りをつないだもの。
- **ボタンと押したときの引数**: リスナーは教えないので、OS の通知の DB（`%LOCALAPPDATA%\Microsoft\Windows\Notifications\wpndatabase.db`。リスナーと同じ ID）から元の XML（トーストの XML）を読む。SQLite は Windows に入っている `winsqlite3.dll`（`raw-dylib`）で、読むたびに読み取り専用で開いて閉じる。リスナーに見えた時点で DB にも書かれている（実測）。
- **出すボタン**: 右クリックのメニュー（`placement`）・文字を打つ欄の横のボタン（`hint-inputId`）・OS のボタン（`system`）は出さない。URL を開くもの（`protocol`）は `http` / `https` だけを `url` で渡す（開くのは画面で、`ui::link` で確かめてから。それ以外のスキームは開けない決まり）。引数で動くもの（`foreground` / `background`）は、アプリが通知の受け口を持っているときだけ出す。
- **押したとき**: アプリの通知の受け口（`INotificationActivationCallback` の COM サーバー。CLSID はレジストリの `AppUserModelId\<ID>\CustomActivator` （Firefox・Office など）か、スタートメニューのショートカットのファイル（.lnk）の `System.AppUserModel.ToastActivatorCLSID`（Chrome・Edge・Discord など。スタートメニューの項目 `shell:AppsFolder` は持っていないので、ショートカットを探す。STA の別スレッドで読む））に、OS が押したときと同じ引数を渡す。受け口が無ければ本文はアプリを開くだけ（`shell:AppsFolder\<ID>`）。届けた通知は通知センターからも消す（OS で押したときと同じ）。
- **アイコン**: 通知のロゴ（ストアのアプリ）。無ければスタートメニューのアイコン（`desktop::app_icon`。Chrome・Discord などのデスクトップのアプリ）。アプリごとに一度だけ読む。

## テスト

`cargo test -p notifications`（読んだ一覧の反映・XML からボタンを読む・押したときの届け方を、決まった結果を返す問い合わせ先で。押したことが本物の COM のローカルサーバー（このプロセスに置き、HKCU に一時的なキーを書いて消す）へ引数ごと届くことは `a_click_reaches_the_apps_activator`）。スタートメニューのショートカットに受け口を書いたアプリがどれも引けることは `activators_are_found_from_start_menu_shortcuts`（`--ignored`）。実機は `cargo test -p notifications -- --ignored --nocapture`（PowerShell の名前でトーストを出し、リスナー・DB・アイコンから読めることを見る。Windows・通知へのアクセスが許されていること）。
