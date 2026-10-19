//! Design と描画バックエンドの間で共有する、ウィンドウごとの材質パラメーター。
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};

/// シェーダーと同じ配置。座標・距離は描画先のピクセル。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Material {
    pub pointers: [[f32; 2]; 2],
    pub translucent: u32,
    pub light: u32,
    pub scale: f32,
    pub padding: u32,
}

thread_local! {
    static WINDOWS: RefCell<HashMap<u64, Material>> = RefCell::new(HashMap::new());
    static CURRENT: Cell<Material> = Cell::new(Material::default());
}

pub fn set(window: u64, material: Material) {
    WINDOWS.with_borrow_mut(|windows| {
        windows.insert(window, material);
    });
}

pub fn remove(window: u64) {
    WINDOWS.with_borrow_mut(|windows| {
        windows.remove(&window);
    });
}

pub fn current() -> Material {
    CURRENT.get()
}

/// 同じスレッドの複数ウィンドウ・入れ子の描画でも、光源を取り違えない。
pub fn with_window<R>(window: u64, draw: impl FnOnce() -> R) -> R {
    struct Restore(Material);
    impl Drop for Restore {
        fn drop(&mut self) {
            CURRENT.set(self.0);
        }
    }
    let material = WINDOWS.with_borrow(|windows| windows.get(&window).copied().unwrap_or_default());
    let _restore = Restore(CURRENT.replace(material));
    draw()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolates_windows_and_restores_after_nested_draws_and_unwind() {
        let a = Material {
            light: 1,
            scale: 0.5,
            ..Material::default()
        };
        let b = Material {
            translucent: 1,
            scale: 2.,
            ..Material::default()
        };
        set(1, a);
        set(2, b);
        with_window(1, || {
            assert_eq!(current(), a);
            let _ = std::panic::catch_unwind(|| {
                with_window(2, || {
                    assert_eq!(current(), b);
                    panic!("描画失敗");
                })
            });
            assert_eq!(current(), a);
        });
        assert_eq!(current(), Material::default());
        remove(1);
        with_window(1, || assert_eq!(current(), Material::default()));
        remove(2);
        assert_eq!(std::mem::size_of::<Material>(), 32);
    }
}
