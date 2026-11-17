//! Windows: 通知センターの一覧は `UserNotificationListener`（パッケージの ID が無いアプリでも読める。ユーザーが
//! 設定で通知へのアクセスを切っていれば読めない）。ボタンと押したときの引数はリスナーが教えないので、OS の通知の DB
//! （`wpndatabase.db`。リスナーと同じ ID）から元の XML を読む。押したときは OS と同じく、アプリの通知の受け口
//! （`INotificationActivationCallback` の COM サーバー）に引数を渡す。受け口が無ければアプリを開くだけ。

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

/// Windows の時刻（1601 年からの 100ns）と UNIX 時刻の差（100ns）。
const EPOCH_DIFF: i64 = 116_444_736_000_000_000;

pub(crate) struct Listener {
    listener: UserNotificationListener,
    /// 最後に読んだ通知（アイコンを読むため）。
    notes: HashMap<u32, UserNotification>,
    /// アプリごとの通知の受け口（COM のクラス）。無いものも覚える。
    activators: HashMap<String, Option<GUID>>,
}

impl Listener {
    pub(crate) fn new() -> Option<Self> {
        // WinRT（と COM）はスレッドごとに初期化が要る（このワーカーのスレッドだけで使う）。
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

/// テスト用の通知を出す名前（PowerShell。どの Windows にも登録されている。nyaw は通知を出すための ID を登録していない）。
// ponytail: デバッグのテスト専用。nyaw から本番の通知を出すなら、インストーラーでショートカットに AppUserModelId を付ける。
const TEST_SENDER: &str =
    r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe";

/// テスト用の通知を OS に出す。押すと開くものは、どれもウェブの URL（`protocol`）。
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

/// XML の文字・属性に入れられるようにする。
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// 押したことを、アプリの通知の受け口に渡す（OS が通知を押したときと同じ。アプリが前に出られるようにしてから）。
fn invoke(clsid: GUID, app: &str, args: &str) -> windows::core::Result<()> {
    unsafe {
        let callback: INotificationActivationCallback =
            CoCreateInstance(&clsid, None, CLSCTX_LOCAL_SERVER)?;
        let _ = CoAllowSetForegroundWindow(&callback, None);
        callback.Activate(&HSTRING::from(app), &HSTRING::from(args), &[])
    }
}

/// アプリを開く（スタートメニューから開くのと同じ。パッケージのアプリもデスクトップのアプリも開ける）。
fn launch(app: &str) -> windows::core::Result<()> {
    std::process::Command::new("explorer.exe")
        .arg(format!("shell:AppsFolder\\{app}"))
        .spawn()
        .map(drop)
        .map_err(|e| windows::core::Error::new(windows::core::HRESULT(-1), e.to_string()))
}

/// レジストリに登録した通知の受け口（`AppUserModelId\<ID>\CustomActivator`。Firefox・Office など）。
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

/// スタートメニューのショートカット（.lnk）に書いた通知の受け口（`System.AppUserModel.ToastActivatorCLSID`。Chrome・Edge・
/// Discord など）。スタートメニューの項目（`shell:AppsFolder`）は持っていないので、ショートカットのファイルを探す。
/// シェルは STA で読むので、呼ぶ側のスレッドによらないよう別のスレッドで読む。結果はアプリごとに覚える（[`Listener::activator`]）。
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

/// ショートカットの AppUserModelId と、書いてあれば通知の受け口。
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

/// スタートメニュー（全ユーザーと自分）のショートカットのファイル。
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

/// ストリームの中身を全部読む。
fn read_stream(reference: &IRandomAccessStreamReference) -> Option<Vec<u8>> {
    let stream = reference.OpenReadAsync().ok()?.get().ok()?;
    let size = u32::try_from(stream.Size().ok()?).ok()?;
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size).ok()?.get().ok()?;
    let mut bytes = vec![0; size as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    Some(bytes)
}

/// OS の通知の DB（SQLite）を Windows に入っている `winsqlite3.dll` で読む（読むだけ。書いているのは OS）。
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

    /// 通知 `id` の元の XML。開いたままにせず、読むたびに開いて閉じる（OS の書き込みを妨げない）。
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
            // 開けなかったときも、確保されたものを返すために閉じる。
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

    /// アプリの通知の受け口の代わり: 受け取ったもの（AppUserModelId と引数）を覚える。
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

    /// 押したことが、OS と同じ経路でアプリの通知の受け口（COM のローカルサーバー）に届く: レジストリの
    /// `AppUserModelId\<ID>\CustomActivator` から受け口を引き、AppUserModelId と通知の引数を渡す。受け口はこの
    /// プロセスに置き（`CoRegisterClassObject`）、レジストリには一時的なキーを HKCU に書いて消す。
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

    /// 実機の確認: PowerShell の名前でトーストを出し、リスナーが読めること・DB からボタンが読めることを見る。
    /// `cargo test -p notifications -- --ignored --nocapture`（Windows。通知へのアクセスが許されていること）。
    #[test]
    #[ignore]
    fn reads_a_real_toast() {
        let mut listener = Listener::new().expect("notification access");
        let before = listener.read().unwrap().len();
        // テスト用の通知を出す経路（`show`）で出す。文字は XML に入れても崩れないこと（& < >）も見る。
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
        assert!(shell.is_some(), "スタートメニューのアイコン");
        listener.remove(raw.id);
    }

    /// 実機の確認: スタートメニューのショートカットに通知の受け口を書いたアプリ（Chrome・Edge・Discord など）は、どれも
    /// AppUserModelId から受け口が引ける（スタートメニューの項目からは引けず、ショートカットを探す）。
    /// `cargo test -p notifications -- --ignored --nocapture`（そういうアプリが 1 つも無ければ何も確かめない）。
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
