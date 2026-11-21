//! Windows: 通知センターの一覧は `UserNotificationListener` で取得する（パッケージ ID を持たないアプリでも取得可能。
//! ユーザーが設定で通知アクセスを無効にしている場合は取得不可）。ボタンと押下時の引数はリスナーが提供しないため、OS の
//! 通知 DB（`wpndatabase.db`、リスナーと同一 ID）から元の XML を読み込む。押下時は OS と同様に、アプリの通知
//! アクティベーター（`INotificationActivationCallback` の COM サーバー）に引数を渡す。アクティベーターが存在しなければアプリを起動するのみ。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows::{
    Data::Xml::Dom::XmlDocument,
    Foundation::Size,
    Storage::Streams::{DataReader, IRandomAccessStreamReference},
    UI::Notifications::{
        KnownNotificationBindings,
        Management::{UserNotificationListener, UserNotificationListenerAccessStatus as Access},
        NotificationKinds, ToastNotification, ToastNotificationManager, UserNotification,
    },
    Win32::{
        Storage::EnhancedStorage::{PKEY_AppUserModel_ID, PKEY_AppUserModel_ToastActivatorCLSID},
        System::{
            Com::{
                CLSCTX_LOCAL_SERVER, CLSIDFromString, COINIT_APARTMENTTHREADED,
                CoAllowSetForegroundWindow, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
            },
            Registry::{HKEY_CLASSES_ROOT, RRF_RT_REG_SZ, RegGetValueW},
            WinRT::{RO_INIT_MULTITHREADED, RoInitialize},
        },
        UI::{
            Notifications::INotificationActivationCallback,
            Shell::{IShellItem2, SHCreateItemFromParsingName},
        },
    },
    core::{GUID, HSTRING, PCWSTR, w},
};

use super::{Backend, Raw};

/// Windows 時刻（1601 年基点、100ns 単位）と UNIX 時刻の差分（100ns 単位）。
const EPOCH_DIFF: i64 = 116_444_736_000_000_000;

pub(crate) struct Listener {
    listener: UserNotificationListener,
    /// 最後に取得した通知（アイコン取得用）。
    notes: HashMap<u32, UserNotification>,
    /// アプリごとの通知アクティベーター（COM クラス）。不在の場合もキャッシュする。
    activators: HashMap<String, Option<GUID>>,
}

impl Listener {
    pub(crate) fn new() -> Option<Self> {
        // WinRT（および COM）はスレッドごとの初期化が必要（このワーカースレッドでのみ使用する）。
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        let listener = UserNotificationListener::Current().ok()?;
        let mut access = listener.GetAccessStatus().ok()?;
        if access == Access::Unspecified {
            access = listener
                .RequestAccessAsync()
                .and_then(|a| a.get())
                .unwrap_or(access);
        }
        if access != Access::Allowed {
            logger::info!("notification access is not allowed", access = ?access);
            return None;
        }
        Some(Self {
            listener,
            notes: HashMap::new(),
            activators: HashMap::new(),
        })
    }

    fn activator(&mut self, app: &str) -> Option<GUID> {
        *self
            .activators
            .entry(app.to_owned())
            .or_insert_with(|| registered(app).or_else(|| shortcut(app)))
    }
}

impl Backend for Listener {
    fn read(&mut self) -> Option<Vec<Raw>> {
        let list = self
            .listener
            .GetNotificationsAsync(NotificationKinds::Toast)
            .ok()?
            .get()
            .ok()?;
        let generic = KnownNotificationBindings::ToastGeneric().ok()?;
        self.notes.clear();
        let mut raws = Vec::new();
        for note in list {
            let Ok(id) = note.Id() else { continue };
            let app = note.AppInfo().ok();
            let app_id = app
                .as_ref()
                .and_then(|a| a.AppUserModelId().ok())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let app_name = app
                .and_then(|a| a.DisplayInfo().ok())
                .and_then(|d| d.DisplayName().ok())
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| app_id.clone());
            let texts = note
                .Notification()
                .and_then(|n| n.Visual())
                .and_then(|v| v.GetBinding(&generic))
                .and_then(|b| b.GetTextElements())
                .map(|t| {
                    t.into_iter()
                        .filter_map(|t| t.Text().ok())
                        .map(|t| t.to_string())
                        .collect()
                })
                .unwrap_or_default();
            let ticks = note.CreationTime().map_or(0, |t| t.UniversalTime) - EPOCH_DIFF;
            let arrived = UNIX_EPOCH + Duration::from_nanos(ticks.max(0) as u64 * 100);
            raws.push(Raw {
                id,
                app_id,
                app_name,
                texts,
                arrived: arrived.min(SystemTime::now()),
            });
            self.notes.insert(id, note);
        }
        Some(raws)
    }

    fn payload(&mut self, id: u32) -> Option<String> {
        db::payload(id)
    }

    fn icon(&mut self, id: u32) -> Option<Vec<u8>> {
        let logo = self
            .notes
            .get(&id)?
            .AppInfo()
            .and_then(|a| a.DisplayInfo())
            .and_then(|d| {
                d.GetLogo(Size {
                    Width: 64.0,
                    Height: 64.0,
                })
            })
            .ok()?;
        read_stream(&logo.into())
    }

    fn invokable(&mut self, app: &str) -> bool {
        self.activator(app).is_some()
    }

    fn activate(&mut self, app: &str, args: Option<&str>) -> bool {
        let done = match (self.activator(app), args) {
            (Some(clsid), args) => invoke(clsid, app, args.unwrap_or("")),
            (None, None) => launch(app),
            (None, Some(_)) => return false,
        };
        if let Err(e) = &done {
            logger::warn!(
                "cannot deliver the notification to the app",
                app = app,
                error = e
            );
        }
        done.is_ok()
    }

    fn remove(&mut self, id: u32) {
        if let Err(e) = self.listener.RemoveNotification(id) {
            logger::warn!("cannot remove the notification", id = id, error = e);
        }
    }

    fn send(&mut self, toast: &super::Outgoing) {
        if let Err(e) = show(toast) {
            logger::warn!("cannot show the test notification", error = e);
        }
    }
}

/// テスト用の通知送信元名（PowerShell。全 Windows に登録済み。nyaw は通知送信用 ID を登録していない）。
// ponytail: デバッグ・テスト専用。nyaw から本番の通知を送信する場合はインストーラーでショートカットに AppUserModelId を付与する。
const TEST_SENDER: &str =
    r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe";

/// テスト用の通知を OS に送信する。押下で開くリンクはすべてウェブ URL（`protocol`）。
fn show(toast: &super::Outgoing) -> windows::core::Result<()> {
    let buttons: String = toast
        .buttons
        .iter()
        .map(|(label, url)| {
            format!(
                r#"<action content="{}" activationType="protocol" arguments="{}"/>"#,
                escape(label),
                escape(url)
            )
        })
        .collect();
    let xml = format!(
        r#"<toast activationType="protocol" launch="{}"><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual><actions>{buttons}</actions></toast>"#,
        escape(&toast.url),
        escape(&toast.title),
        escape(&toast.body),
    );
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(xml))?;
    let notification = ToastNotification::CreateToastNotification(&doc)?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(TEST_SENDER))?
        .Show(&notification)
}

/// XML のテキスト・属性に安全に埋め込めるようエスケープする。
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// 押下をアプリの通知アクティベーターに通知する（OS がトースト押下時に行う処理と同一。事前にアプリへのフォアグラウンド許可を設定する）。
fn invoke(clsid: GUID, app: &str, args: &str) -> windows::core::Result<()> {
    unsafe {
        let callback: INotificationActivationCallback =
            CoCreateInstance(&clsid, None, CLSCTX_LOCAL_SERVER)?;
        let _ = CoAllowSetForegroundWindow(&callback, None);
        callback.Activate(&HSTRING::from(app), &HSTRING::from(args), &[])
    }
}

/// アプリを起動する（スタートメニューからの起動と同一。パッケージアプリ・デスクトップアプリの両方に対応する）。
fn launch(app: &str) -> windows::core::Result<()> {
    std::process::Command::new("explorer.exe")
        .arg(format!("shell:AppsFolder\\{app}"))
        .spawn()
        .map(drop)
        .map_err(|e| windows::core::Error::new(windows::core::HRESULT(-1), e.to_string()))
}

/// レジストリに登録された通知アクティベーター（`AppUserModelId\<ID>\CustomActivator`。Firefox・Office 等）。
fn registered(app: &str) -> Option<GUID> {
    let key = HSTRING::from(format!("AppUserModelId\\{app}"));
    let mut buf = [0u16; 64];
    let mut len = std::mem::size_of_val(&buf) as u32;
    unsafe {
        RegGetValueW(
            HKEY_CLASSES_ROOT,
            &key,
            w!("CustomActivator"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
        .ok()
        .ok()?;
        CLSIDFromString(PCWSTR(buf.as_ptr())).ok()
    }
}

/// スタートメニューのショートカット（.lnk）に記録された通知アクティベーター（`System.AppUserModel.ToastActivatorCLSID`。
/// Chrome・Edge・Discord 等）。スタートメニューの項目（`shell:AppsFolder`）には格納されないため、ショートカットファイルを検索する。
/// シェルは STA で読み込むため、呼び出し側スレッドに依存しないよう別スレッドで実行する。結果はアプリごとにキャッシュする（[`Listener::activator`]）。
fn shortcut(app: &str) -> Option<GUID> {
    std::thread::scope(|s| {
        s.spawn(|| {
            let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            start_menu_links()
                .iter()
                .find_map(|path| match link_activator(path) {
                    Some((id, clsid)) if id == app => clsid,
                    _ => None,
                })
        })
        .join()
        .ok()
        .flatten()
    })
}

/// ショートカットの AppUserModelId と、記録されていれば通知アクティベーターを返す。
fn link_activator(path: &Path) -> Option<(String, Option<GUID>)> {
    let item: IShellItem2 =
        unsafe { SHCreateItemFromParsingName(&HSTRING::from(path), None) }.ok()?;
    let id = unsafe { item.GetString(&PKEY_AppUserModel_ID) }.ok()?;
    let text = unsafe { id.to_string() }.ok();
    unsafe { CoTaskMemFree(Some(id.0 as *const _)) };
    let clsid = unsafe { item.GetCLSID(&PKEY_AppUserModel_ToastActivatorCLSID) }
        .ok()
        .filter(|c| *c != GUID::zeroed());
    Some((text?, clsid))
}

/// スタートメニュー（全ユーザーおよびカレントユーザー）のショートカットファイル一覧を返す。
fn start_menu_links() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = ["ProgramData", "APPDATA"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(|root| Path::new(&root).join(r"Microsoft\Windows\Start Menu\Programs"))
        .collect();
    let mut links = Vec::new();
    while let Some(dir) = dirs.pop() {
        for path in std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
        {
            if path.is_dir() {
                dirs.push(path);
            } else if path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("lnk"))
            {
                links.push(path);
            }
        }
    }
    links
}

/// ストリームの内容を全量読み出す。
fn read_stream(reference: &IRandomAccessStreamReference) -> Option<Vec<u8>> {
    let stream = reference.OpenReadAsync().ok()?.get().ok()?;
    let size = u32::try_from(stream.Size().ok()?).ok()?;
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size).ok()?.get().ok()?;
    let mut bytes = vec![0; size as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    Some(bytes)
}

/// OS の通知 DB（SQLite）を Windows 標準の `winsqlite3.dll` で読み込む（読み取り専用。書き込みは OS が行う）。
mod db {
    use std::ffi::{CString, c_char, c_int, c_void};
    use std::ptr::null_mut;

    #[link(name = "winsqlite3", kind = "raw-dylib")]
    unsafe extern "system" {
        fn sqlite3_open_v2(
            path: *const c_char,
            db: *mut *mut c_void,
            flags: c_int,
            vfs: *const c_char,
        ) -> c_int;
        fn sqlite3_prepare_v2(
            db: *mut c_void,
            sql: *const c_char,
            len: c_int,
            stmt: *mut *mut c_void,
            tail: *mut *const c_char,
        ) -> c_int;
        fn sqlite3_bind_int64(stmt: *mut c_void, index: c_int, value: i64) -> c_int;
        fn sqlite3_step(stmt: *mut c_void) -> c_int;
        fn sqlite3_column_blob(stmt: *mut c_void, column: c_int) -> *const c_void;
        fn sqlite3_column_bytes(stmt: *mut c_void, column: c_int) -> c_int;
        fn sqlite3_finalize(stmt: *mut c_void) -> c_int;
        fn sqlite3_close(db: *mut c_void) -> c_int;
    }

    const OPEN_READONLY: c_int = 1;
    const OK: c_int = 0;
    const ROW: c_int = 100;

    /// 通知 `id` の元の XML を返す。開いたまま保持せず、取得のたびに開閉する（OS の書き込みをブロックしない）。
    pub(super) fn payload(id: u32) -> Option<String> {
        let dir = std::env::var_os("LOCALAPPDATA")?;
        let path =
            std::path::Path::new(&dir).join(r"Microsoft\Windows\Notifications\wpndatabase.db");
        let path = CString::new(path.to_str()?).ok()?;
        let sql = c"SELECT Payload FROM Notification WHERE Id = ?1";
        let mut out = None;
        unsafe {
            let mut db = null_mut();
            if sqlite3_open_v2(path.as_ptr(), &mut db, OPEN_READONLY, std::ptr::null()) == OK {
                let mut stmt = null_mut();
                if sqlite3_prepare_v2(db, sql.as_ptr(), -1, &mut stmt, null_mut()) == OK
                    && sqlite3_bind_int64(stmt, 1, id.into()) == OK
                    && sqlite3_step(stmt) == ROW
                {
                    let (p, n) = (sqlite3_column_blob(stmt, 0), sqlite3_column_bytes(stmt, 0));
                    if !p.is_null() && n > 0 {
                        let bytes = std::slice::from_raw_parts(p.cast::<u8>(), n as usize);
                        out = String::from_utf8(bytes.to_vec()).ok();
                    }
                }
                sqlite3_finalize(stmt);
            }
            // オープン失敗時でも、確保済みリソースを解放するため必ず閉じる。
            sqlite3_close(db);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use windows::Win32::System::Com::{
        COINIT_MULTITHREADED, CoInitializeEx, CoRegisterClassObject, CoRevokeClassObject,
        IClassFactory, IClassFactory_Impl, REGCLS_MULTIPLEUSE,
    };
    use windows::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RegDeleteTreeW, RegSetKeyValueW,
    };
    use windows::Win32::UI::Notifications::{
        INotificationActivationCallback_Impl, NOTIFICATION_USER_INPUT_DATA,
    };
    use windows::core::{BOOL, IUnknown, Interface, Ref, implement};

    type Got = Arc<Mutex<Vec<(String, String)>>>;

    /// アプリの通知アクティベーターの代替: 受信内容（AppUserModelId と引数）を記録する。
    #[implement(INotificationActivationCallback)]
    struct Callback(Got);

    impl INotificationActivationCallback_Impl for Callback_Impl {
        fn Activate(
            &self,
            app: &PCWSTR,
            args: &PCWSTR,
            _: *const NOTIFICATION_USER_INPUT_DATA,
            _: u32,
        ) -> windows::core::Result<()> {
            let got = unsafe { (app.to_string()?, args.to_string()?) };
            self.0.lock().unwrap().push(got);
            Ok(())
        }
    }

    #[implement(IClassFactory)]
    struct Factory(Got);

    impl IClassFactory_Impl for Factory_Impl {
        fn CreateInstance(
            &self,
            _: Ref<'_, IUnknown>,
            riid: *const GUID,
            out: *mut *mut core::ffi::c_void,
        ) -> windows::core::Result<()> {
            let callback: IUnknown = Callback(self.0.clone()).into();
            unsafe { callback.query(riid, out).ok() }
        }

        fn LockServer(&self, _: BOOL) -> windows::core::Result<()> {
            Ok(())
        }
    }

    /// 押下が OS と同一経路でアプリの通知アクティベーター（COM ローカルサーバー）に到達する: レジストリの
    /// `AppUserModelId\<ID>\CustomActivator` からアクティベーターを解決し、AppUserModelId と通知引数を渡す。
    /// アクティベーターはこのプロセスに登録し（`CoRegisterClassObject`）、レジストリには一時キーを HKCU に作成・削除する。
    #[test]
    fn a_click_reaches_the_apps_activator() {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let clsid = GUID::new().unwrap();
        let app = format!("nyaw.test.{clsid:?}");
        let got: Got = Arc::default();
        let factory: IClassFactory = Factory(got.clone()).into();
        let cookie = unsafe {
            CoRegisterClassObject(&clsid, &factory, CLSCTX_LOCAL_SERVER, REGCLS_MULTIPLEUSE)
        }
        .unwrap();
        let key = HSTRING::from(format!("Software\\Classes\\AppUserModelId\\{app}"));
        let value: Vec<u16> = format!("{{{clsid:?}}}").encode_utf16().chain([0]).collect();
        let written = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                &key,
                w!("CustomActivator"),
                REG_SZ.0,
                Some(value.as_ptr().cast()),
                (value.len() * 2) as u32,
            )
        };
        let found = registered(&app);
        let delivered = found.map(|clsid| invoke(clsid, &app, "action=reply&id=7"));
        unsafe {
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &key);
            let _ = CoRevokeClassObject(cookie);
        }
        assert!(written.is_ok(), "{written:?}");
        assert_eq!(found, Some(clsid), "the activator is found in the registry");
        delivered.unwrap().unwrap();
        assert_eq!(
            *got.lock().unwrap(),
            [(app, "action=reply&id=7".to_owned())]
        );
    }

    /// 実機検証: PowerShell の名前でトーストを送信し、リスナーが取得できること・DB からボタンを読み込めることを確認する。
    /// `cargo test -p notifications -- --ignored --nocapture`（Windows。通知アクセスが許可されていること）。
    #[test]
    #[ignore]
    fn reads_a_real_toast() {
        let mut listener = Listener::new().expect("notification access");
        let before = listener.read().unwrap().len();
        // テスト用の通知送信経路（`show`）で送信する。文字列が XML エスケープで破損しないこと（& < >）も確認する。
        show(&super::super::Outgoing {
            title: "nyaw & <probe>".into(),
            body: "probe".into(),
            url: "https://example.com/".into(),
            buttons: vec![("Site".into(), "https://example.com/a?b=1&c=2".into())],
        })
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
        let raws = listener.read().unwrap();
        assert!(raws.len() > before, "{} -> {}", before, raws.len());
        let raw = raws.iter().max_by_key(|r| r.id).unwrap();
        assert_eq!(raw.texts, ["nyaw & <probe>", "probe"]);
        let (launch, actions) = super::super::parse(&listener.payload(raw.id).unwrap(), false);
        assert_eq!(
            launch,
            super::super::Plan::Url("https://example.com/".into())
        );
        assert_eq!(
            actions,
            [(
                "Site".to_owned(),
                super::super::Plan::Url("https://example.com/a?b=1&c=2".into())
            )]
        );
        let shell = desktop::app_icon(&raw.app_id).map(|i| (i.width, i.height));
        println!(
            "{raw:?} logo={:?} shell={shell:?}",
            listener.icon(raw.id).map(|b| b.len())
        );
        assert!(
            shell.is_some(),
            "スタートメニューのアイコンが取得できること"
        );
        listener.remove(raw.id);
    }

    /// 実機検証: スタートメニューのショートカットに通知アクティベーターを記録しているアプリ（Chrome・Edge・Discord 等）は、
    /// すべて AppUserModelId からアクティベーターを解決できる（スタートメニューの項目からは解決不可のため、ショートカットを検索する）。
    /// `cargo test -p notifications -- --ignored --nocapture`（該当アプリが存在しなければ検証をスキップする）。
    #[test]
    #[ignore]
    fn activators_are_found_from_start_menu_shortcuts() {
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let declared: Vec<(String, GUID)> = start_menu_links()
            .iter()
            .filter_map(|p| link_activator(p))
            .filter_map(|(id, clsid)| Some((id, clsid?)))
            .collect();
        println!("{} shortcuts declare an activator", declared.len());
        for (id, clsid) in declared {
            let found = shortcut(&id);
            println!("{id} -> {found:?}");
            assert!(found.is_some(), "{id}: {clsid:?}");
        }
    }
}
