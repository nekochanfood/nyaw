use super::*;
use gpui::{Bounds, Hsla, PathBuilder, Pixels, Point, canvas, fill, point, size};
use kernel::vr::{HandSkeleton, JOINT_COUNT, PALM, TIPS, joint_parent};

/// 親指の先とほかの指の先がこれより近ければ、つまんでいるとみなす（m）。
// ponytail: しきい値 1 つでヒステリシスが無い。境目で点滅して困るなら、離すしきい値を別に持つ。
pub const PINCH_DISTANCE: f32 = 0.02;
/// 手の図に収める範囲（手のひらを中心にした一辺、m）。
const VIEW_M: f32 = 0.25;
/// 関節のギズモの軸の長さ（m）。
const AXIS_M: f32 = 0.012;
const HAND_PX: f32 = 440.0;
/// ギズモの軸の色（X 赤・Y 緑・Z 青）。
const AXIS_COLORS: [u32; 3] = [0xe5484dff, 0x30a46cff, 0x3e63ddff];

/// 親指とつまむ指（人差し指・中指・薬指・小指）の名前のキーと、手ごとの表示の id。
const FINGERS: [(&str, [&str; 2]); 4] = [
    ("skeleton.index", ["pinch-left-index", "pinch-right-index"]),
    (
        "skeleton.middle",
        ["pinch-left-middle", "pinch-right-middle"],
    ),
    ("skeleton.ring", ["pinch-left-ring", "pinch-right-ring"]),
    (
        "skeleton.little",
        ["pinch-left-little", "pinch-right-little"],
    ),
];

/// 親指の先とほかの指の先（[`TIPS`] の 1〜4 番目）がつまんでいるか。
pub fn pinches(skeleton: &HandSkeleton) -> [bool; 4] {
    std::array::from_fn(|i| skeleton.pinch_distance(TIPS[i + 1]) < PINCH_DISTANCE)
}

/// スケルタル入力を試す画面（`skeleton-test`）。左右の手（`skeleton-left` / `skeleton-right`）の関節を、
/// 頭から見た向きで手のひらを中心に描く。関節ごとに向きのギズモ（X 赤・Y 緑・Z 青）、骨は親の関節との線。
/// 親指の先とほかの指の先が [`PINCH_DISTANCE`] より近いと、その指の表示（`pinch-{left,right}-{index,middle,ring,little}`）と骨が光る。
/// 骨格が取れないランタイム（novr・拡張の無い OpenXR）では手が出ない。毎フレーム描き直す（[`ui::PanelState::live`]）。
pub struct SkeletonTest {}

impl SkeletonTest {
    pub fn new(panel: PanelId, cx: &mut App) -> Self {
        cx.default_global::<PanelControl>()
            .panels
            .entry(panel)
            .or_default()
            .live = true;
        Self {}
    }
}

impl Render for SkeletonTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let (hands, hmd) = cx
            .try_global::<PanelControl>()
            .map_or((Vec::new(), Pose::IDENTITY), |c| (c.hands.clone(), c.hmd));
        let column = |side: usize, hand: Hand, title: &'static str| {
            let skeleton = hands.iter().find(|s| s.hand == hand).copied();
            let pinched = skeleton.as_ref().map_or([false; 4], pinches);
            let rows = FINGERS
                .iter()
                .zip(pinched)
                .enumerate()
                .map(|(i, ((name, ids), on))| {
                    let label = match &skeleton {
                        Some(s) => i18n::tf(
                            cx,
                            "skeleton.pinch",
                            &[
                                ("finger", &i18n::t(cx, name)),
                                ("mm", &format!("{:.0}", s.pinch_distance(TIPS[i + 1]) * 1e3)),
                            ],
                        ),
                        None => i18n::t(cx, name),
                    };
                    indicator(&theme, ids[side], label, on)
                });
            labeled(
                &theme,
                i18n::t(cx, title),
                div()
                    .flex()
                    .flex_col()
                    .gap(px(spacing::SM))
                    .child(
                        div()
                            .id(["skeleton-left", "skeleton-right"][side])
                            .debug_selector(move || ["skeleton-left", "skeleton-right"][side].into())
                            .size(px(HAND_PX))
                            .rounded(px(radius::MD))
                            .bg(rgba(theme.control))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(rgba(theme.neutral_text_low))
                            .map(|d| match skeleton {
                                Some(s) => {
                                    let colors = (
                                        Hsla::from(rgba(theme.neutral_text_low)),
                                        Hsla::from(rgba(theme.accent_solid)),
                                    );
                                    d.child(
                                        canvas(
                                            |_, _, _| {},
                                            move |b, _, window, _| {
                                                paint_hand(b, &s, hmd, pinched, colors, window)
                                            },
                                        )
                                        .size_full(),
                                    )
                                }
                                None => d.child(i18n::t(cx, "skeleton.untracked")),
                            }),
                    )
                    .children(rows),
            )
        };
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap(px(spacing::LG))
            .p(px(spacing::XL))
            .text_color(rgba(theme.neutral_text))
            .child(
                div()
                    .text_color(rgba(theme.neutral_text_low))
                    .child(i18n::t(cx, "skeleton.instructions")),
            )
            .child(
                row()
                    .gap(px(spacing::XL))
                    .items_start()
                    .child(column(0, Hand::Left, "skeleton.left"))
                    .child(column(1, Hand::Right, "skeleton.right")),
            )
    }
}

/// 手のひらを `b` の中心に、頭（`hmd`）から見た向きで正射影して描く。骨・関節の点・関節ごとの向きのギズモ。
/// つまんでいる指（`pinched`）は親指と一緒に `colors.1` で描く（ほかの骨は `colors.0`）。
fn paint_hand(
    b: Bounds<Pixels>,
    s: &HandSkeleton,
    hmd: Pose,
    pinched: [bool; 4],
    (bone, lit): (Hsla, Hsla),
    window: &mut Window,
) {
    let view = hmd.orientation.inverse();
    let palm = s.joints[PALM].position;
    let scale = f32::from(b.size.width) / VIEW_M;
    let center = b.center();
    let to_screen = |p: Vec3| -> Point<Pixels> {
        let v = view * (p - palm);
        point(center.x + px(v.x * scale), center.y - px(v.y * scale))
    };
    // 関節が属する指（0 親指〜4 小指。手のひら・手首は無し）。
    let finger = |j: usize| (j >= 2).then(|| (j - 1) / 5);
    let lit_joint = |j: usize| match finger(j) {
        Some(0) => pinched.iter().any(|&p| p),
        Some(f) => pinched[f - 1],
        None => false,
    };
    let line = |window: &mut Window, from: Vec3, to: Vec3, width: f32, color: Hsla| {
        let mut path = PathBuilder::stroke(px(width));
        path.move_to(to_screen(from));
        path.line_to(to_screen(to));
        if let Ok(path) = path.build() {
            window.paint_path(path, color);
        }
    };
    for j in 0..JOINT_COUNT {
        if let Some(parent) = joint_parent(j) {
            let color = if lit_joint(j) { lit } else { bone };
            line(
                window,
                s.joints[parent].position,
                s.joints[j].position,
                3.0,
                color,
            );
        }
    }
    for (j, joint) in s.joints.iter().enumerate() {
        for (axis, color) in [Vec3::X, Vec3::Y, Vec3::Z].into_iter().zip(AXIS_COLORS) {
            let tip = joint.position + joint.orientation * axis * AXIS_M;
            line(window, joint.position, tip, 2.0, rgba(color).into());
        }
        let r = if j == PALM { 6.0 } else { 4.0 };
        let at = to_screen(joint.position);
        window.paint_quad(
            fill(
                Bounds::new(point(at.x - px(r), at.y - px(r)), size(px(r * 2.0), px(r * 2.0))),
                if lit_joint(j) { lit } else { bone },
            )
            .corner_radii(px(r)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 指先を親指の先に寄せた指だけがつまんでいる。
    #[test]
    fn pinch_is_per_finger() {
        let mut s = HandSkeleton {
            hand: Hand::Right,
            joints: [Pose::IDENTITY; JOINT_COUNT],
        };
        for (i, &tip) in TIPS.iter().enumerate() {
            s.joints[tip].position = Vec3::new(i as f32 * 0.03, 0.0, 0.0);
        }
        assert_eq!(pinches(&s), [false; 4]);
        s.joints[TIPS[2]].position = Vec3::new(0.0, 0.015, 0.0);
        assert_eq!(pinches(&s), [false, true, false, false]);
        s.joints[TIPS[1]].position = Vec3::new(0.019, 0.0, 0.0);
        assert_eq!(pinches(&s), [true, true, false, false]);
    }
}
