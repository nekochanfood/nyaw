//! OpenXR バックエンド: オーバーレイは XR_EXTX_overlay セッション + クアッドレイヤー、
//! 入力はインタラクションプロファイルごとに推奨バインドしたアクションセット。

use crate::{Runtime, VrBackend, VrError};

pub(crate) fn available() -> bool {
    os::openxr_runtime_manifest().is_some()
}

pub(crate) fn connect() -> Result<Box<dyn VrBackend>, VrError> {
    // ponytail: FFI 未接続。次は `openxr` クレート (feature "loaded") で XR_EXTX_overlay を有効化し、
    // 現在のインタラクションプロファイルのパスから "/interaction_profiles/" を除いたものを RawController.profile にする。
    Err(VrError::NotImplemented(Runtime::OpenXr))
}
