//! OpenVR (SteamVR) バックエンド: オーバーレイは `IVROverlay`、入力は `IVRSystem` のコントローラ状態。

use crate::{Runtime, VrBackend, VrError};

pub(crate) fn available() -> bool {
    os::openvr_paths_file().is_some()
}

pub(crate) fn connect() -> Result<Box<dyn VrBackend>, VrError> {
    // ponytail: FFI 未接続。次は `openvr_sys` で VR_Init(VRApplication_Overlay) し、
    // Prop_ControllerType_String ("knuckles" -> "valve/index_controller" 等) を RawController に写す。
    Err(VrError::NotImplemented(Runtime::OpenVr))
}
