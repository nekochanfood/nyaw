//! Steam Audio（phonon）の最小の FFI。ライブラリは実行時に読み、無ければ呼び出し側がパンニングに落とす。
//! bindgen（clang）をビルドに要求しないよう、使う型と関数だけを `phonon.h`（4.8）から写している。

use glam::Vec3;
use libloading::Library;
use std::ffi::{c_char, c_void};
use std::ptr;

type Handle = *mut c_void;

/// 要求する API の版（`STEAMAUDIO_VERSION`）。ライブラリは同じメジャーで、マイナー・パッチがこれ以上なら受け付ける。
const VERSION: u32 = (4 << 16) | (8 << 8);
const STATUS_SUCCESS: i32 = 0;
const SIMD_AVX2: i32 = 3;
const HRTF_DEFAULT: i32 = 0;
const HRTF_NORM_NONE: i32 = 0;
const INTERPOLATION_NEAREST: i32 = 0;

#[repr(C)]
struct ContextSettings {
    version: u32,
    log_callback: Option<unsafe extern "system" fn(i32, *const c_char)>,
    allocate_callback: *const c_void,
    free_callback: *const c_void,
    simd_level: i32,
    flags: i32,
}

#[repr(C)]
struct AudioSettings {
    sampling_rate: i32,
    frame_size: i32,
}

#[repr(C)]
struct HrtfSettings {
    kind: i32,
    sofa_file_name: *const c_char,
    sofa_data: *const u8,
    sofa_data_size: i32,
    volume: f32,
    norm_type: i32,
}

#[repr(C)]
struct BinauralEffectSettings {
    hrtf: Handle,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Vector3 {
    x: f32,
    y: f32,
    z: f32,
}

#[repr(C)]
struct BinauralEffectParams {
    direction: Vector3,
    interpolation: i32,
    spatial_blend: f32,
    hrtf: Handle,
    peak_delays: *mut f32,
}

#[repr(C)]
struct AudioBuffer {
    num_channels: i32,
    num_samples: i32,
    data: *mut *mut f32,
}

type ContextCreate = unsafe extern "system" fn(*mut ContextSettings, *mut Handle) -> i32;
type Release = unsafe extern "system" fn(*mut Handle);
type HrtfCreate =
    unsafe extern "system" fn(Handle, *mut AudioSettings, *mut HrtfSettings, *mut Handle) -> i32;
type BinauralCreate = unsafe extern "system" fn(
    Handle,
    *mut AudioSettings,
    *mut BinauralEffectSettings,
    *mut Handle,
) -> i32;
type BinauralReset = unsafe extern "system" fn(Handle);
type BinauralApply = unsafe extern "system" fn(
    Handle,
    *mut BinauralEffectParams,
    *mut AudioBuffer,
    *mut AudioBuffer,
) -> i32;

/// 読み込んだ Steam Audio と、コンテキスト・HRTF。
pub(crate) struct Phonon {
    context: Handle,
    hrtf: Handle,
    settings: (i32, i32),
    context_release: Release,
    hrtf_release: Release,
    binaural_create: BinauralCreate,
    binaural_release: Release,
    binaural_reset: BinauralReset,
    binaural_apply: BinauralApply,
    // 関数ポインタより後に落とす（宣言順に Drop される）。
    _lib: Library,
}

// SAFETY: Steam Audio のコンテキスト・HRTF・エフェクトはスレッドをまたいで使ってよい（同時に呼ばない限り）。
// Phonon はミキサーが単独で所有し、一度に 1 スレッドからしか触らない。
unsafe impl Send for Phonon {}

/// 音源 1 つぶんの HRTF の畳み込みの状態。[`Phonon::binaural`] で作り、同じ `Phonon` に返して解放する。
pub(crate) struct Binaural(Handle);

// SAFETY: Phonon と同じ。
unsafe impl Send for Binaural {}

/// ライブラリを探す名前。`NYAW_PHONON` で場所を指せる。無ければ OS の既定の探し方（Windows は実行ファイルの隣が最初）。
fn library_name() -> std::ffi::OsString {
    std::env::var_os("NYAW_PHONON").unwrap_or_else(|| {
        if cfg!(windows) {
            "phonon.dll".into()
        } else {
            "libphonon.so".into()
        }
    })
}

unsafe extern "system" fn log(level: i32, message: *const c_char) {
    // SAFETY: Steam Audio は NUL 終端の文字列を渡す。
    let message = unsafe { std::ffi::CStr::from_ptr(message) }.to_string_lossy();
    match level {
        2 => logger::error!("steam audio", message = message.trim_end()),
        1 => logger::warn!("steam audio", message = message.trim_end()),
        _ => logger::debug!("steam audio", message = message.trim_end()),
    }
}

impl Phonon {
    /// Steam Audio を読み、既定の HRTF を作る。`frame_size` は [`Self::apply`] に渡す 1 回ぶんのサンプル数。
    pub(crate) fn load(sampling_rate: u32, frame_size: usize) -> Result<Self, String> {
        let name = library_name();
        // SAFETY: Steam Audio のライブラリを読むだけ（初期化で副作用のある処理を持たない）。
        let lib = unsafe { Library::new(&name) }
            .map_err(|e| format!("{}: {e}", name.to_string_lossy()))?;
        // SAFETY: シンボルの型は phonon.h の宣言どおり。
        unsafe {
            let sym = |n: &[u8]| -> Result<*const c_void, String> {
                lib.get::<*const c_void>(n)
                    .map(|s| *s)
                    .map_err(|e| format!("{}: {e}", String::from_utf8_lossy(n)))
            };
            let context_create: ContextCreate = std::mem::transmute(sym(b"iplContextCreate\0")?);
            let context_release: Release = std::mem::transmute(sym(b"iplContextRelease\0")?);
            let hrtf_create: HrtfCreate = std::mem::transmute(sym(b"iplHRTFCreate\0")?);
            let hrtf_release: Release = std::mem::transmute(sym(b"iplHRTFRelease\0")?);
            let binaural_create: BinauralCreate =
                std::mem::transmute(sym(b"iplBinauralEffectCreate\0")?);
            let binaural_release: Release =
                std::mem::transmute(sym(b"iplBinauralEffectRelease\0")?);
            let binaural_reset: BinauralReset =
                std::mem::transmute(sym(b"iplBinauralEffectReset\0")?);
            let binaural_apply: BinauralApply =
                std::mem::transmute(sym(b"iplBinauralEffectApply\0")?);

            let mut context = ptr::null_mut();
            let mut cs = ContextSettings {
                version: VERSION,
                log_callback: Some(log),
                allocate_callback: ptr::null(),
                free_callback: ptr::null(),
                // AVX512 はクロックが落ちて遅くなることがある（phonon.h の注意書き）。
                simd_level: SIMD_AVX2,
                flags: 0,
            };
            let status = context_create(&mut cs, &mut context);
            if status != STATUS_SUCCESS {
                return Err(format!("iplContextCreate failed: {status}"));
            }
            let settings = (sampling_rate as i32, frame_size as i32);
            let mut audio = AudioSettings {
                sampling_rate: settings.0,
                frame_size: settings.1,
            };
            let mut hs = HrtfSettings {
                kind: HRTF_DEFAULT,
                sofa_file_name: ptr::null(),
                sofa_data: ptr::null(),
                sofa_data_size: 0,
                volume: 1.0,
                norm_type: HRTF_NORM_NONE,
            };
            let mut hrtf = ptr::null_mut();
            let status = hrtf_create(context, &mut audio, &mut hs, &mut hrtf);
            if status != STATUS_SUCCESS {
                context_release(&mut context);
                return Err(format!("iplHRTFCreate failed: {status}"));
            }
            Ok(Self {
                context,
                hrtf,
                settings,
                context_release,
                hrtf_release,
                binaural_create,
                binaural_release,
                binaural_reset,
                binaural_apply,
                _lib: lib,
            })
        }
    }

    pub(crate) fn binaural(&self) -> Option<Binaural> {
        let mut audio = AudioSettings {
            sampling_rate: self.settings.0,
            frame_size: self.settings.1,
        };
        let mut es = BinauralEffectSettings { hrtf: self.hrtf };
        let mut effect = ptr::null_mut();
        // SAFETY: 生きているコンテキストと HRTF を渡す。
        let status =
            unsafe { (self.binaural_create)(self.context, &mut audio, &mut es, &mut effect) };
        (status == STATUS_SUCCESS).then_some(Binaural(effect))
    }

    /// 前の音の残り（畳み込みの尾）を捨てる。別の音に使い回す前に呼ぶ。
    pub(crate) fn reset(&self, effect: &mut Binaural) {
        // SAFETY: このコンテキストで作ったエフェクト。
        unsafe { (self.binaural_reset)(effect.0) }
    }

    pub(crate) fn release(&self, mut effect: Binaural) {
        // SAFETY: このコンテキストで作ったエフェクトを 1 回だけ解放する。
        unsafe { (self.binaural_release)(&mut effect.0) }
    }

    /// モノラルの `input` を、聞き手から見た向き `direction`（聞き手の座標。右 +X・上 +Y・前 -Z の単位ベクトル）から
    /// 聞こえるように HRTF で畳み込み、`left` / `right` に書く。長さはどれも `frame_size`。
    pub(crate) fn apply(
        &self,
        effect: &mut Binaural,
        direction: Vec3,
        input: &mut [f32],
        left: &mut [f32],
        right: &mut [f32],
    ) {
        let n = self.settings.1 as usize;
        debug_assert!(input.len() == n && left.len() == n && right.len() == n);
        let mut params = BinauralEffectParams {
            direction: Vector3 {
                x: direction.x,
                y: direction.y,
                z: direction.z,
            },
            interpolation: INTERPOLATION_NEAREST,
            spatial_blend: 1.0,
            hrtf: self.hrtf,
            peak_delays: ptr::null_mut(),
        };
        let mut in_ptrs = [input.as_mut_ptr()];
        let mut out_ptrs = [left.as_mut_ptr(), right.as_mut_ptr()];
        let mut inb = AudioBuffer {
            num_channels: 1,
            num_samples: n as i32,
            data: in_ptrs.as_mut_ptr(),
        };
        let mut outb = AudioBuffer {
            num_channels: 2,
            num_samples: n as i32,
            data: out_ptrs.as_mut_ptr(),
        };
        // SAFETY: バッファは frame_size ぶんあり、呼び出しの間だけ借りる。
        unsafe { (self.binaural_apply)(effect.0, &mut params, &mut inb, &mut outb) };
    }
}

impl Drop for Phonon {
    fn drop(&mut self) {
        // SAFETY: 作ったものを作った逆順に 1 回ずつ解放する。エフェクトは先に返されている。
        unsafe {
            (self.hrtf_release)(&mut self.hrtf);
            (self.context_release)(&mut self.context);
        }
    }
}
