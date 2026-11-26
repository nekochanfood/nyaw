//! openvr_api の代替: SteamVR の `vrclient` を直接読み込み、`IVRClientCore` を呼び出す。
//! openvr_api は jsoncpp を静的リンクしており、OpenXR ローダー（同じく jsoncpp を含む）と
//! 同時にリンクするとシンボルが衝突するため、必要な処理（数個の関数）のみを自前で実装する。
//! `VR_CONFIG_PATH` / `VR_LOG_PATH` は vrclient 自身が読み込むため、ここでは扱わない。

use std::ffi::{CStr, CString, c_char, c_void};
use std::path::PathBuf;

use libloading::Library;
use openvr_bindings as sys;

use crate::VrError;

/// `vr::IVRClientCore`（ivrclientcore.h）の仮想関数テーブル。順序が ABI そのもの。
/// x64 Windows ではメンバ関数は `this` を第 1 引数とする C 呼び出し規約と同一。
#[repr(C)]
struct CoreVtbl {
    init: unsafe extern "C" fn(
        *mut Core,
        sys::EVRApplicationType,
        *const c_char,
    ) -> sys::EVRInitError,
    cleanup: unsafe extern "C" fn(*mut Core),
    _is_interface_version_valid:
        unsafe extern "C" fn(*mut Core, *const c_char) -> sys::EVRInitError,
    get_generic_interface:
        unsafe extern "C" fn(*mut Core, *const c_char, *mut sys::EVRInitError) -> *mut c_void,
    _is_hmd_present: unsafe extern "C" fn(*mut Core) -> bool,
    english_error: unsafe extern "C" fn(*mut Core, sys::EVRInitError) -> *const c_char,
}

#[repr(C)]
struct Core {
    vtbl: *const CoreVtbl,
}

type Factory = unsafe extern "C" fn(*const c_char, *mut i32) -> *mut c_void;

pub(super) struct Client {
    core: *mut Core,
    initialized: bool,
    // 関数テーブルの参照先のため最後に解放する。
    _lib: Library,
}

impl Client {
    /// `VR_InitInternal` と同等の処理を行う。
    pub fn init(app: sys::EVRApplicationType) -> Result<Self, VrError> {
        let mut client = Self::load()?;
        let err = unsafe { (client.vtbl().init)(client.core, app, std::ptr::null()) };
        if err != sys::EVRInitError_VRInitError_None {
            return Err(client.error("VR_Init", err));
        }
        client.initialized = true;
        Ok(client)
    }

    /// vrclient を読み込んで `IVRClientCore` を取得するのみ（初期化は行わない）。
    /// 同一プロセスですでに `init` 済みであれば、その接続をそのまま使用可能。
    pub fn load() -> Result<Self, VrError> {
        let runtime = runtime_path().ok_or_else(|| {
            VrError::Runtime("SteamVR location unknown (openvrpaths.vrpath)".into())
        })?;
        #[cfg(windows)]
        let dll = runtime.join("bin").join("vrclient_x64.dll");
        let lib = unsafe { Library::new(&dll) }
            .map_err(|e| VrError::Runtime(format!("{}: {e}", dll.display())))?;
        let factory = unsafe { lib.get::<Factory>(b"VRClientCoreFactory\0") }
            .map_err(|e| VrError::Runtime(format!("VRClientCoreFactory: {e}")))?;
        let mut code = 0;
        let core = unsafe { factory(c"IVRClientCore_003".as_ptr(), &mut code) } as *mut Core;
        if core.is_null() {
            return Err(VrError::Runtime(format!(
                "failed to get IVRClientCore_003 ({code})"
            )));
        }
        Ok(Self {
            core,
            initialized: false,
            _lib: lib,
        })
    }

    fn vtbl(&self) -> &CoreVtbl {
        unsafe { &*(*self.core).vtbl }
    }

    pub fn error(&self, what: &str, err: sys::EVRInitError) -> VrError {
        let msg = unsafe { CStr::from_ptr((self.vtbl().english_error)(self.core, err)) };
        VrError::Runtime(format!("{what}: {}", msg.to_string_lossy()))
    }

    /// `version` は `b"IVRSystem_022\0"` 等のバインディング定数。C API の関数テーブルを取得する。
    pub fn interface<T>(&self, version: &[u8]) -> Result<*const T, VrError> {
        let version = CStr::from_bytes_with_nul(version)
            .unwrap()
            .to_str()
            .unwrap();
        let name = CString::new(format!("FnTable:{version}")).unwrap();
        let mut err = sys::EVRInitError_VRInitError_None;
        let table =
            unsafe { (self.vtbl().get_generic_interface)(self.core, name.as_ptr(), &mut err) };
        if err != sys::EVRInitError_VRInitError_None || table.is_null() {
            return Err(self.error(version, err));
        }
        Ok(table as *const T)
    }

    /// `VR_ShutdownInternal` と同等の処理を行う。
    pub fn shutdown(&mut self) {
        if std::mem::take(&mut self.initialized) {
            unsafe { (self.vtbl().cleanup)(self.core) };
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// SteamVR のパス。`VR_OVERRIDE` が設定されていればそれを使用し、なければ `openvrpaths.vrpath` の `runtime` の先頭エントリを返す。
pub fn runtime_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("VR_OVERRIDE").filter(|v| !v.is_empty()) {
        return Some(p.into());
    }
    let json = std::fs::read_to_string(os::openvr_paths_file()?).ok()?;
    first_string(&json, "runtime").map(PathBuf::from)
}

/// `"key" : [ "値", ... ]` の最初の文字列を抽出する。openvrpaths.vrpath の読み込みに特化した最小限の JSON パーサ。
// ponytail: 配列の最初の文字列のみ参照する。他のキーも必要になったら JSON パーサライブラリを導入する。
fn first_string(json: &str, key: &str) -> Option<String> {
    let rest = &json[json.find(&format!("\"{key}\""))? + key.len() + 2..];
    let rest = &rest[rest.find('[')? + 1..];
    let mut chars = rest[rest.find('"')? + 1..].chars();
    let mut out = String::new();
    loop {
        match chars.next()? {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                c => out.push(c), // \\ \" \/
            },
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_first_runtime_from_vrpath() {
        let json = r#"{
	"config" : [ "C:\\Steam\\config" ],
	"runtime" :
	[
		"C:\\Program Files (x86)\\Steam\\steamapps\\common\\SteamVR",
		"D:\\Other"
	],
	"version" : 1
}"#;
        assert_eq!(
            first_string(json, "runtime").as_deref(),
            Some(r"C:\Program Files (x86)\Steam\steamapps\common\SteamVR")
        );
        assert_eq!(first_string(json, "missing"), None);
    }

    #[test]
    fn unescapes_quotes_and_slashes() {
        assert_eq!(
            first_string(r#"{"runtime":["/a\/b \"c\""]}"#, "runtime").as_deref(),
            Some(r#"/a/b "c""#)
        );
    }
}
