//! アプリが作った画像の解放。GPUI は表示した画像をウィンドウごとのアトラス（GPU のテクスチャ）に登録し、
//! `drop_image` を呼ぶまで保持する。画像を差し替えるだけでは GPU メモリーが増え続ける。

use std::sync::{Arc, Mutex};

use gpui::{App, RenderImage};

static IMAGES: Mutex<Vec<Arc<RenderImage>>> = Mutex::new(Vec::new());

/// 画像を登録する。`RenderImage` を作る処理は必ずこの関数を通す。どのスレッドからでも呼べる。
pub fn track(image: Arc<RenderImage>) -> Arc<RenderImage> {
    IMAGES.lock().unwrap().push(image.clone());
    image
}

/// 登録した画像のうち、ほかに参照がないものを全ウィンドウのアトラスから外して手放す。
/// 更新中のウィンドウには適用されないので、ウィンドウの更新の外で呼ぶ。
// ponytail: 呼ぶたびに全件を調べる。画像が数千枚になるなら、解放を通知する型に置き換える。
pub fn sweep(cx: &mut App) {
    let unused: Vec<_> = IMAGES
        .lock()
        .unwrap()
        .extract_if(.., |image| Arc::strong_count(image) == 1)
        .collect();
    for image in unused {
        cx.drop_image(image, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn sweep_releases_only_unused_images(cx: &mut gpui::TestAppContext) {
        let frame = image::Frame::new(image::RgbaImage::new(4, 4));
        let image = track(Arc::new(RenderImage::new(vec![frame])));
        let weak = Arc::downgrade(&image);
        cx.update(sweep);
        assert!(weak.upgrade().is_some(), "an image in use stays");
        drop(image);
        cx.update(sweep);
        assert!(weak.upgrade().is_none(), "an unused image is released");
    }
}
