//! SteamVR のアプリとしての登録（IVRApplications）: vrmanifest を書き出して登録し、SteamVR の起動と同時に開くかを切り替える。
//! 開発版（debug ビルド）はアプリの名前を分離し、リリース版の登録（起動する実行ファイル）を変更しない。

use std::ffi::CString;
use std::path::{Path, PathBuf};

use openvr_bindings as sys;

use super::f;
use crate::VrError;

const APP_KEY: &str = if cfg!(debug_assertions) {
    "nekochanfood.nyaw.dev"
} else {
    "nekochanfood.nyaw"
};
const APP_NAME: &str = if cfg!(debug_assertions) {
    "nyaw (dev)"
} else {
    "nyaw"
};

pub(super) struct App {
    table: *const sys::VR_IVRApplications_FnTable,
    /// SteamVR の起動と同時に起動するか。毎フレーム読み取られるため、問い合わせは接続時のみ。切り替え後は設定した値を保持する
    /// （SteamVR は切り替え直後の `GetApplicationAutoLaunch` で前の値を返す。次の接続では新しい値が読み取れる）。
    auto_launch: bool,
}

fn key() -> CString {
    CString::new(APP_KEY).unwrap()
}

fn check(what: &str, e: sys::EVRApplicationError) -> Result<(), VrError> {
    if e == sys::EVRApplicationError_VRApplicationError_None {
        Ok(())
    } else {
        Err(VrError::Runtime(format!("{what}: EVRApplicationError {e}")))
    }
}

impl App {
    /// 登録済みであれば、この接続をそのアプリとして識別させる（手動起動でも SteamVR 起動でも、バインドが同じアプリに紐付く）。
    /// SteamVR Input の manifest より前に呼ぶこと。
    pub fn init(table: *const sys::VR_IVRApplications_FnTable) -> Self {
        let t = unsafe { &*table };
        let key = key();
        let installed = unsafe { f(t.IsApplicationInstalled)(key.as_ptr().cast_mut()) };
        if installed
            && let Err(e) = check("IdentifyApplication", unsafe {
                f(t.IdentifyApplication)(0, key.as_ptr().cast_mut())
            })
        {
            logger::warn!("cannot identify as the registered SteamVR app", error = e);
        }
        let mut app = Self {
            table,
            auto_launch: false,
        };
        app.auto_launch = app.read_auto_launch();
        app
    }

    pub fn auto_launch(&self) -> bool {
        self.auto_launch
    }

    /// 有効化時は、この実行ファイルを起動する vrmanifest を書き出して再登録してから有効化する。無効化時は登録を残す（識別を維持する）。
    pub fn set_auto_launch(&mut self, on: bool) -> Result<(), VrError> {
        let t = unsafe { &*self.table };
        if on {
            let path = register_manifest(t)?;
            logger::info!(
                "registered nyaw as a SteamVR app",
                manifest = path.display()
            );
        }
        check("SetApplicationAutoLaunch", unsafe {
            f(t.SetApplicationAutoLaunch)(key().as_ptr().cast_mut(), on)
        })?;
        // 再読み取りしない: 直後は前の値が返り、トグルが動作しなくなる。
        self.auto_launch = on;
        Ok(())
    }

    fn read_auto_launch(&self) -> bool {
        unsafe { f((*self.table).GetApplicationAutoLaunch)(key().as_ptr().cast_mut()) }
    }
}

/// `os::config_dir()/openvr/<アプリの名前>.vrmanifest` に書き出し、SteamVR に恒久的に登録する。
fn register_manifest(t: &sys::VR_IVRApplications_FnTable) -> Result<PathBuf, VrError> {
    let exe = std::env::current_exe().map_err(|e| VrError::Runtime(format!("current_exe: {e}")))?;
    let path = os::config_dir()
        .ok_or_else(|| VrError::Runtime("config directory unknown".into()))?
        .join("openvr")
        .join(format!("{APP_KEY}.vrmanifest"));
    os::write_file(&path, &manifest(&exe))
        .map_err(|e| VrError::Runtime(format!("{}: {e}", path.display())))?;
    let c = CString::new(path.to_string_lossy().as_ref()).unwrap();
    check("AddApplicationManifest", unsafe {
        f(t.AddApplicationManifest)(c.as_ptr().cast_mut(), false)
    })?;
    Ok(path)
}

/// SteamVR が実行ファイルのパスを読むキー（OS ごとに別）。
const BINARY_PATH_KEY: &str = if cfg!(windows) {
    "binary_path_windows"
} else {
    "binary_path_linux"
};

/// SteamVR のアプリの定義。`is_dashboard_overlay` が無いと自動起動を切り替えられない。
fn manifest(exe: &Path) -> String {
    // str の Debug は `\` と `"` をエスケープするので、パスはそのまま JSON の文字列になる。
    let exe = format!("{:?}", exe.to_string_lossy());
    format!(
        r#"{{
  "source": "builtin",
  "applications": [
    {{
      "app_key": "{APP_KEY}",
      "launch_type": "binary",
      "{BINARY_PATH_KEY}": {exe},
      "arguments": "--openvr",
      "is_dashboard_overlay": true,
      "strings": {{ "en_us": {{ "name": "{APP_NAME}" }} }}
    }}
  ]
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_launches_this_exe_on_openvr() {
        let exe = Path::new(r#"C:\Users\ねこ\Programs\nyaw\nyaw.exe"#);
        let json: serde_json::Value = serde_json::from_str(&manifest(exe)).unwrap();
        let app = &json["applications"][0];
        assert_eq!(app["app_key"], APP_KEY);
        assert_eq!(app[BINARY_PATH_KEY], exe.to_str().unwrap());
        assert_eq!(app["arguments"], "--openvr");
        assert_eq!(app["is_dashboard_overlay"], true);
    }

    /// SteamVR を真似る関数表: 登録済みで自動起動は入っており、`GetApplicationAutoLaunch` は切り替えても前の値（入）を
    /// 返し続ける。`SetApplicationAutoLaunch` は [`SET_ERROR`] を返す。
    static SET_ERROR: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
    unsafe extern "C" fn yes(_: *mut std::ffi::c_char) -> bool {
        true
    }
    unsafe extern "C" fn identify(_: u32, _: *mut std::ffi::c_char) -> sys::EVRApplicationError {
        sys::EVRApplicationError_VRApplicationError_None
    }
    unsafe extern "C" fn set(_: *mut std::ffi::c_char, _: bool) -> sys::EVRApplicationError {
        SET_ERROR.load(std::sync::atomic::Ordering::Relaxed) as _
    }

    /// 切り替えたらすぐスイッチに出る（SteamVR が直後に前の値を返しても）。切り替えに失敗したら前の値のまま。
    #[test]
    fn switching_shows_at_once_though_steamvr_still_reads_the_old_value() {
        // 関数ポインタの Option はゼロで None になる（呼ばない関数は空のまま）。
        let mut table: sys::VR_IVRApplications_FnTable = unsafe { std::mem::zeroed() };
        table.IsApplicationInstalled = Some(yes);
        table.IdentifyApplication = Some(identify);
        table.GetApplicationAutoLaunch = Some(yes);
        table.SetApplicationAutoLaunch = Some(set);
        let mut app = App::init(&table);
        assert!(app.auto_launch());
        let unknown = sys::EVRApplicationError_VRApplicationError_UnknownApplication;
        SET_ERROR.store(unknown as _, std::sync::atomic::Ordering::Relaxed);
        assert!(app.set_auto_launch(false).is_err());
        assert!(app.auto_launch());
        SET_ERROR.store(0, std::sync::atomic::Ordering::Relaxed);
        app.set_auto_launch(false).unwrap();
        assert!(!app.auto_launch());
    }

    #[test]
    fn dev_builds_register_as_a_separate_app() {
        assert_eq!(APP_KEY.ends_with(".dev"), cfg!(debug_assertions));
    }
}
