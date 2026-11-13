//! SteamVR のアプリとしての登録（IVRApplications）: vrmanifest を書き出して登録し、SteamVR の起動と同時に開くかを切り替える。
//! 開発版（debug ビルド）はアプリの名前を分け、リリース版の登録（起動する実行ファイル）に触らない。

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
    /// SteamVR の起動と同時に開くか。毎フレーム読まれるので、接続時と切り替えたときだけ問い合わせる。
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
    /// 登録済みなら、この接続をそのアプリだと名乗る（手で起動しても SteamVR が起動しても、バインドが同じアプリに付く）。
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

    /// 入れるときは、この実行ファイルを起動する vrmanifest を書き出して登録し直してから入れる。切るときは登録を残す（名乗りを保つ）。
    pub fn set_auto_launch(&mut self, on: bool) -> Result<(), VrError> {
        let t = unsafe { &*self.table };
        if on {
            let path = register_manifest(t)?;
            logger::info!(
                "registered nyaw as a SteamVR app",
                manifest = path.display()
            );
        }
        let r = check("SetApplicationAutoLaunch", unsafe {
            f(t.SetApplicationAutoLaunch)(key().as_ptr().cast_mut(), on)
        });
        self.auto_launch = self.read_auto_launch();
        r
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
      "binary_path_windows": {exe},
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
        assert_eq!(app["binary_path_windows"], exe.to_str().unwrap());
        assert_eq!(app["arguments"], "--openvr");
        assert_eq!(app["is_dashboard_overlay"], true);
    }

    #[test]
    fn dev_builds_register_as_a_separate_app() {
        assert_eq!(APP_KEY.ends_with(".dev"), cfg!(debug_assertions));
    }
}
