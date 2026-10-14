//! Layer 6 画面（Web）: コントローラのビーム（`beam` の Web 版）。手ごとに、手元から伸びるレーザーと、
//! 指しているパネルに貼り付く小さな円（カーソル）の 2 枚。見た目だけを描き、置き場所・長さ・表示はオーバーレイ層が毎 tick 決める。
// ponytail: ドラッグの見た目（`Pointer::Drag`）は無い。Web のドラッグはページ内で要素を動かして見せる。

use kernel::overlay::LASER_MAX;
use kernel::{Hand, Pointer, Pose, Vec2};
use webui::{Page, WebPanelSpec};

/// レーザーのテクスチャ。横方向がレーザーの向きで、オーバーレイ層が幅 = 長さに伸ばす（`beam` と同じ寸法）。
const LASER_PX: (u32, u32) = (1024, 64);
/// カーソルの直径（m）。
const CURSOR_M: f32 = 0.012;
/// 非表示ウィンドウには最小の大きさがあるので小さくしすぎない。
const CURSOR_PX: (u32, u32) = (160, 160);

pub fn laser() -> Page {
    Page::html("laser", include_str!("../web/laser.html"))
}

pub fn cursor() -> Page {
    Page::html("cursor", include_str!("../web/cursor.html"))
}

/// 両手のレーザーとカーソル。
pub fn specs() -> Vec<WebPanelSpec> {
    [Hand::Left, Hand::Right]
        .into_iter()
        .flat_map(|hand| {
            [
                WebPanelSpec::new(Pose::IDENTITY, laser())
                    .sized(Vec2::new(LASER_MAX, LASER_MAX / 16.0), LASER_PX)
                    .pointer(hand, Pointer::Laser)
                    .transparent(),
                WebPanelSpec::new(Pose::IDENTITY, cursor())
                    .sized(Vec2::splat(CURSOR_M), CURSOR_PX)
                    .pointer(hand, Pointer::Cursor)
                    .transparent(),
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_hands_get_a_laser_and_a_cursor() {
        let names: Vec<_> = specs().iter().map(|s| s.page().name()).collect();
        assert_eq!(names, ["laser", "cursor", "laser", "cursor"]);
    }
}
