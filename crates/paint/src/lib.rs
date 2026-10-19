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

/// 読み書きの範囲が重ならない連続した部品は、背面の退避をまとめられる。
/// 各矩形はぼかしの届く余白を含む [左, 上, 右, 下]。
pub fn batch_len<T>(items: &[T], bounds: impl Fn(&T) -> [f32; 4]) -> usize {
    let Some(first) = items.first() else {
        return 0;
    };
    let mut union = bounds(first);
    for (index, item) in items.iter().enumerate().skip(1) {
        let b = bounds(item);
        if b[0] < union[2] && b[2] > union[0] && b[1] < union[3] && b[3] > union[1] {
            return index;
        }
        union = [
            union[0].min(b[0]),
            union[1].min(b[1]),
            union[2].max(b[2]),
            union[3].max(b[3]),
        ];
    }
    items.len()
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
    fn batches_only_independent_reads_and_writes() {
        let rectangles = [[0., 0., 10., 10.], [12., 0., 22., 10.], [20., 0., 30., 10.]];
        assert_eq!(batch_len(&rectangles, |b| *b), 2);
        assert_eq!(batch_len(&rectangles[..2], |b| *b), 2);
        assert_eq!(batch_len(&rectangles[..0], |b| *b), 0);
    }

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
