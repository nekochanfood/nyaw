//! Layer 6 画面: ダッシュボード。左手の X ボタンを二回続けて押すと開き、もう一度二回押すと閉じる
//! （開け閉めは常駐の wrist が [`kernel::PanelStatus::dashboard`] を見て行う）。今は中央に 🐟 を置いただけのパネル。

use design::Theme;
use gpui::{Context, Window, div, prelude::*, px};
use kernel::vr::{Pose, Quat, Vec2, Vec3};
use ui::PanelSpec;

/// 開いた瞬間の頭から見て正面 0.7m、少し下。
pub const POSE: Pose = Pose::new(Vec3::new(0.0, -0.05, -0.7), Quat::IDENTITY);
const SIZE_M: Vec2 = Vec2::new(0.64, 0.4);
const RES_PX: (u32, u32) = (640, 400);
/// 🐟 の一辺（px）。
const FISH_PX: f32 = 160.0;

/// 開くとき・閉じるときの音。閉じるパネルの音は止まるので、開け閉めする側（wrist）のスピーカーで鳴らす。
pub const OPEN_SOUND: &[u8] = include_bytes!("../assets/dashboard_open.wav");
pub const CLOSE_SOUND: &[u8] = include_bytes!("../assets/dashboard_close.wav");

/// ダッシュボードの起動仕様。`opened` は作られたパネルの ID を受け取る。
pub fn spec(opened: impl FnOnce(kernel::PanelId, &mut gpui::App) + 'static) -> PanelSpec {
    PanelSpec::new(Pose::IDENTITY, move |id, _, cx| {
        opened(id, cx);
        Dashboard
    })
    .sized(SIZE_M, RES_PX)
    .in_view(POSE)
    .titled("dashboard.title")
}

pub struct Dashboard;

impl Render for Dashboard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        design::components::card_panel(&theme)
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("dashboard-fish")
                    .debug_selector(|| "dashboard-fish".into())
                    .child(design::emoji("🐟").size(px(FISH_PX))),
            )
    }
}
