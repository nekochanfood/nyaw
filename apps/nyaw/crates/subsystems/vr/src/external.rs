//! 表示サーバーが書き込んだ画像を、CPU で並べ替えずにランタイムの GPU へ取り込むための型
//! （[`VrBackend::submit_overlay_external`](crate::VrBackend::submit_overlay_external)）。

use std::{fmt, sync::Mutex};

/// 外部の画像の画素の並び。名前はメモリ上のバイト順（DRM の fourcc とは逆順になる）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    /// B, G, R, A（`DRM_FORMAT_ARGB8888`）。
    Bgra,
    /// B, G, R, 未使用（`DRM_FORMAT_XRGB8888`）。不透明として扱う。
    Bgrx,
    /// R, G, B, A（`DRM_FORMAT_ABGR8888`）。
    Rgba,
    /// R, G, B, 未使用（`DRM_FORMAT_XBGR8888`）。不透明として扱う。
    Rgbx,
}

impl PixelFormat {
    pub const ALL: [PixelFormat; 4] = [Self::Bgra, Self::Bgrx, Self::Rgba, Self::Rgbx];

    /// DRM の fourcc（`drm_fourcc.h`）。
    pub const fn fourcc(self) -> u32 {
        const fn code(s: &[u8; 4]) -> u32 {
            u32::from_le_bytes(*s)
        }
        match self {
            Self::Bgra => code(b"AR24"),
            Self::Bgrx => code(b"XR24"),
            Self::Rgba => code(b"AB24"),
            Self::Rgbx => code(b"XB24"),
        }
    }

    pub fn from_fourcc(fourcc: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.fourcc() == fourcc)
    }

    /// 4 バイト目を透明度として使わない。
    pub const fn opaque(self) -> bool {
        matches!(self, Self::Bgrx | Self::Rgbx)
    }

    /// B と R が入れ替わっている（RGBA へ並べ替えが要る）。
    pub const fn bgr(self) -> bool {
        matches!(self, Self::Bgra | Self::Bgrx)
    }
}

/// 外部の画像の置き場所。
pub enum Memory {
    /// DMA-BUF（単一プレーン）。`fd` はこの値が所有する。
    #[cfg(target_os = "linux")]
    DmaBuf {
        fd: std::os::fd::OwnedFd,
        /// DRM の修飾子（タイリングや圧縮の種類）。
        modifier: u64,
        offset: u32,
        stride: u32,
    },
    /// CPU から読める共有メモリ（XSHM、wl_shm、PipeWire の memfd）。`ptr` と `len` は
    /// [`ImportFormats::host_alignment`] の倍数に揃っていること。画像は `ptr + offset` から始まる。
    Host {
        ptr: *const u8,
        len: usize,
        offset: usize,
        stride: u32,
    },
}

/// 表示サーバーの画像。画像を読み終えるまで書き込み元へ返却しないため、ランタイムは GPU の読み出しが終わるまで
/// この値を保持する。破棄すると返却する（[`Self::new`] の `release`）。
pub struct ExternalImage {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub memory: Memory,
    release: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

// Host の指す共有メモリは、`release` を呼ぶまで書き込み元が書き換えない（読み出しのみ）。
unsafe impl Send for ExternalImage {}
unsafe impl Sync for ExternalImage {}

impl ExternalImage {
    /// 破棄時に `release` を呼ぶ（書き込み元へバッファを返す）。
    pub fn new(
        width: u32,
        height: u32,
        format: PixelFormat,
        memory: Memory,
        release: impl FnOnce() + Send + 'static,
    ) -> Self {
        Self {
            width,
            height,
            format,
            memory,
            release: Mutex::new(Some(Box::new(release))),
        }
    }

    /// 共有メモリであれば、行間を含む画素（`stride` × `height`）。DMA-BUF は CPU から読まない。
    pub fn host_bytes(&self) -> Option<(&[u8], usize)> {
        match self.memory {
            Memory::Host {
                ptr,
                offset,
                stride,
                ..
            } => {
                let len = stride as usize * self.height as usize;
                // SAFETY: 書き込み元は `release` まで `ptr + offset` から `len` を有効に保つ（Memory::Host）。
                Some((
                    unsafe { std::slice::from_raw_parts(ptr.add(offset), len) },
                    stride as usize,
                ))
            }
            #[cfg(target_os = "linux")]
            Memory::DmaBuf { .. } => None,
        }
    }
}

impl Drop for ExternalImage {
    fn drop(&mut self) {
        if let Some(release) = self.release.get_mut().ok().and_then(Option::take) {
            release();
        }
    }
}

impl fmt::Debug for ExternalImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.memory {
            Memory::Host { .. } => "host",
            #[cfg(target_os = "linux")]
            Memory::DmaBuf { .. } => "dmabuf",
        };
        f.debug_struct("ExternalImage")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("format", &self.format)
            .field("memory", &kind)
            .finish()
    }
}

/// 同じ画像（同じバッファ）かどうかだけを比べる。
impl PartialEq for ExternalImage {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}
impl Eq for ExternalImage {}

/// ランタイムが取り込める外部の画像（[`VrBackend::import_formats`](crate::VrBackend::import_formats)）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportFormats {
    /// 取り込める DMA-BUF の画素の並びと、それぞれの修飾子。
    pub dmabuf: Vec<(PixelFormat, Vec<u64>)>,
    /// 共有メモリを取り込めるとき、その先頭と長さの境界（バイト）。
    pub host_alignment: Option<usize>,
}

impl ImportFormats {
    pub fn modifiers(&self, format: PixelFormat) -> &[u64] {
        self.dmabuf
            .iter()
            .find(|(f, _)| *f == format)
            .map_or(&[], |(_, m)| m)
    }

    /// `ptr`・`len` の共有メモリを取り込めるか。
    pub fn host(&self, ptr: *const u8, len: usize) -> bool {
        self.host_alignment
            .is_some_and(|a| a > 0 && (ptr as usize).is_multiple_of(a) && len.is_multiple_of(a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    #[test]
    fn fourcc_round_trips_and_marks_padding_as_opaque() {
        assert_eq!(PixelFormat::Bgrx.fourcc(), 0x3432_5258);
        for f in PixelFormat::ALL {
            assert_eq!(PixelFormat::from_fourcc(f.fourcc()), Some(f));
        }
        assert!(PixelFormat::Bgrx.opaque() && !PixelFormat::Bgra.opaque());
        assert!(PixelFormat::Bgra.bgr() && !PixelFormat::Rgbx.bgr());
    }

    #[test]
    fn dropping_the_image_returns_the_buffer_once() {
        let released = Arc::new(AtomicBool::new(false));
        let flag = released.clone();
        let data = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let image = Arc::new(ExternalImage::new(
            1,
            2,
            PixelFormat::Bgra,
            Memory::Host {
                ptr: data.as_ptr(),
                len: data.len(),
                offset: 0,
                stride: 4,
            },
            move || assert!(!flag.swap(true, Ordering::AcqRel)),
        ));
        assert_eq!(image.host_bytes().unwrap(), (&data[..], 4));
        let held = image.clone();
        drop(image);
        assert!(!released.load(Ordering::Acquire));
        drop(held);
        assert!(released.load(Ordering::Acquire));
    }

    #[test]
    fn host_memory_must_be_aligned_to_import() {
        let formats = ImportFormats {
            host_alignment: Some(4096),
            ..Default::default()
        };
        assert!(formats.host(4096 as *const u8, 8192));
        assert!(!formats.host(4100 as *const u8, 8192));
        assert!(!formats.host(4096 as *const u8, 100));
        assert!(!ImportFormats::default().host(4096 as *const u8, 4096));
    }
}
