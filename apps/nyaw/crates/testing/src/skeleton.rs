use super::*;
use gpui::{Bounds, Hsla, PathBuilder, Pixels, Point, canvas, fill, point, size};
use kernel::ControllerState;
use kernel::subsystems::input::{HandMetrics, HandTuning, PRESS};
use kernel::vr::{Facing, FingerTracking, HandSkeleton, JOINT_COUNT, PALM, TIPS, joint_parent};

/// 親指先端と他の指先端がこの距離未満であればピンチとみなす（m）。画面上の指別表示にのみ使用する
/// （操作判定は入力層の [`HandTuning`] が担当）。
// ponytail: しきい値が単一でヒステリシスなし。境界でのちらつきが問題なら、リリース用しきい値を別途設ける。
pub const PINCH_DISTANCE: f32 = 0.02;
/// 手の描画範囲（手のひら中心の一辺、m）。
const VIEW_M: f32 = 0.25;
/// 関節ギズモの軸の長さ（m）。
const AXIS_M: f32 = 0.012;
const HAND_PX: f32 = 360.0;
/// 画面のサイズ（m）と解像度（px）。手の描画 2 つと判定表示を収容する。
pub const SKELETON_SIZE: Vec2 = Vec2::new(1.1, 1.08);
pub const SKELETON_PX: (u32, u32) = (1100, 1080);
/// チューニング画面のサイズ（m）と解像度（px）。
pub const TUNING_SIZE: Vec2 = Vec2::new(0.8, 0.98);
pub const TUNING_PX: (u32, u32) = (800, 980);
/// ギズモの軸色（X: 赤、Y: 緑、Z: 青）。
const AXIS_COLORS: [u32; 3] = [0xe5484dff, 0x30a46cff, 0x3e63ddff];

/// 親指とピンチ対象の指（人差し指・中指・薬指・小指）の i18n キーと、手別の表示 ID。
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

/// 手別の判定表示 ID（入力ソース・クリック・グラブ・ショートカット）。
const SOURCE_IDS: [&str; 2] = ["input-source-left", "input-source-right"];
const CLICK_IDS: [&str; 2] = ["hand-click-left", "hand-click-right"];
const GRAB_IDS: [&str; 2] = ["hand-grab-left", "hand-grab-right"];
const SHORTCUT_IDS: [&str; 2] = ["hand-shortcut-left", "hand-shortcut-right"];

/// 親指先端と他の指先端（[`TIPS`] の 1〜4 番目）がピンチ状態かを判定する。
pub fn pinches(skeleton: &HandSkeleton) -> [bool; 4] {
    std::array::from_fn(|i| skeleton.pinch_distance(TIPS[i + 1]) < PINCH_DISTANCE)
}

/// ハイライトする指（親指〜小指）。骨格のみで操作する手は、有効な操作に対応する指（クリック = 親指と人差し指、
/// グラブ = 中指・薬指・小指、ショートカット = 親指と小指）。それ以外はピンチ中の指と親指。
fn lit_fingers(skeleton: &HandSkeleton, state: Option<&ControllerState>) -> [bool; 5] {
    match state.filter(|c| c.metrics.is_some()) {
        Some(c) => [
            c.select || c.shortcut,
            c.select,
            c.grab,
            c.grab,
            c.grab || c.shortcut,
        ],
        None => {
            let p = pinches(skeleton);
            [p.iter().any(|&p| p), p[0], p[1], p[2], p[3]]
        }
    }
}

/// 関節が属する指のインデックス（0: 親指〜4: 小指。手のひら・手首は None）。
fn finger_of(joint: usize) -> Option<usize> {
    (joint >= 2).then(|| (joint - 1) / 5)
}

fn hand_state(cx: &App, hand: Hand) -> Option<ControllerState> {
    cx.try_global::<PanelControl>()?
        .controllers
        .iter()
        .find(|c| c.hand == hand)
        .copied()
}

/// スケルタル入力のテスト画面（`skeleton-test`）。左右の手（`skeleton-left` / `skeleton-right`）の関節を、
/// HMD 視点で手のひら中心に描画する。関節ごとに方向ギズモ（X: 赤、Y: 緑、Z: 青）、骨は親関節への線で表現する。
/// 手ごとに操作判定を表示: 入力ソース（`input-source-*`: 骨格のみ / コントローラ / なし）、クリック / グラブ / ショートカット
/// （`hand-click-*` / `hand-grab-*` / `hand-shortcut-*`: 有効中はハイライトし、判定に使用した強度を表示）、
/// 手のひら / 手の甲の向き（`facing-*`）、指別ピンチ（`pinch-{left,right}-{index,middle,ring,little}`）、
/// ダッシュボード開閉回数（`double-press-count`: X ボタンのダブルプレスおよび手の長押し）。しきい値は `hand-tuning` で開く [`HandTuningTest`] で調整する。
/// 骨格データを取得できないランタイム（novr・拡張のない OpenXR）では手は表示されない。
/// 毎フレーム再描画する（[`ui::PanelState::live`]）。
pub struct SkeletonTest {
    panel: PanelId,
}

impl SkeletonTest {
    pub fn new(panel: PanelId, cx: &mut App) -> Self {
        cx.default_global::<PanelControl>()
            .panels
            .entry(panel)
            .or_default()
            .live = true;
        Self { panel }
    }
}

/// 強度（0〜1）をパーセント文字列に変換する。
fn percent(v: f32) -> String {
    format!("{:.0}", v * 100.0)
}

impl Render for SkeletonTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let (hands, hmd) = cx
            .try_global::<PanelControl>()
            .map_or((Vec::new(), Pose::IDENTITY), |c| (c.hands.clone(), c.hmd));
        let tuning = settings(cx).hand;
        let double_press = cx
            .try_global::<PanelControl>()
            .and_then(|c| c.panels.get(&self.panel))
            .map_or(0, |p| p.status.double_press);
        let column = |side: usize, hand: Hand, title: &'static str| {
            let skeleton = hands.iter().find(|s| s.hand == hand).copied();
            let state = hand_state(cx, hand);
            let metrics: Option<HandMetrics> = state.and_then(|c| c.metrics);
            let pinched = skeleton.as_ref().map_or([false; 4], pinches);
            let lit = skeleton
                .as_ref()
                .map_or([false; 5], |s| lit_fingers(s, state.as_ref()));
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
            // 入力ソースと骨格のフィンガートラッキング精度（完全であれば、コントローラとして検出されていても骨格で操作する）。
            let tracking = i18n::t(
                cx,
                match skeleton.map(|s| s.tracking) {
                    Some(FingerTracking::Full) => "skeleton.tracking_full",
                    Some(FingerTracking::Partial) => "skeleton.tracking_partial",
                    Some(FingerTracking::Estimated) => "skeleton.tracking_estimated",
                    None => "skeleton.untracked",
                },
            );
            let source = indicator(
                &theme,
                SOURCE_IDS[side],
                i18n::tf(
                    cx,
                    match (state, metrics) {
                        (Some(_), Some(_)) => "skeleton.source_hand",
                        (Some(_), None) => "skeleton.source_controller",
                        (None, _) => "skeleton.source_none",
                    },
                    &[("tracking", &tracking)],
                ),
                metrics.is_some(),
            );
            // 強度は判定に使用した値。押下しきい値は [`PRESS`]（60%）。コントローラ・手が存在しなければ状態のみ表示。
            let strength = |key: &str, v: Option<f32>, on: bool, id: &'static str| {
                let v = v.map_or_else(|| "-".into(), percent);
                indicator(
                    &theme,
                    id,
                    i18n::tf(cx, key, &[("pct", &v), ("press", &percent(PRESS))]),
                    on,
                )
            };
            let palm_ok =
                metrics.is_some_and(|m| m.palm_toward >= tuning.palm_deg.to_radians().cos());
            let shortcut = indicator(
                &theme,
                SHORTCUT_IDS[side],
                i18n::tf(
                    cx,
                    "skeleton.shortcut",
                    &[
                        (
                            "pct",
                            &metrics.map_or_else(|| "-".into(), |m| percent(m.little)),
                        ),
                        (
                            "palm",
                            &i18n::t(
                                cx,
                                if palm_ok {
                                    "skeleton.palm_ok"
                                } else {
                                    "skeleton.palm_ng"
                                },
                            ),
                        ),
                    ],
                ),
                state.is_some_and(|c| c.shortcut),
            );
            let facing = skeleton.map(|s| s.facing(hmd.position));
            let facing = indicator(
                &theme,
                ["facing-left", "facing-right"][side],
                i18n::t(
                    cx,
                    match facing {
                        Some(Facing::Palm) => "skeleton.palm",
                        Some(Facing::Back) => "skeleton.back",
                        None => "skeleton.untracked",
                    },
                ),
                facing == Some(Facing::Palm),
            );
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
                            .debug_selector(move || {
                                ["skeleton-left", "skeleton-right"][side].into()
                            })
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
                                                paint_hand(b, &s, hmd, lit, colors, window)
                                            },
                                        )
                                        .size_full(),
                                    )
                                }
                                None => d.child(i18n::t(cx, "skeleton.untracked")),
                            }),
                    )
                    .child(source)
                    .child(strength(
                        "skeleton.click",
                        metrics.map(|m| m.pinch),
                        state.is_some_and(|c| c.select),
                        CLICK_IDS[side],
                    ))
                    .child(strength(
                        "skeleton.grab",
                        metrics.map(|m| m.grip),
                        state.is_some_and(|c| c.grab),
                        GRAB_IDS[side],
                    ))
                    .child(shortcut)
                    .child(facing)
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
                    .child(
                        div()
                            .debug_selector(|| "double-press-count".into())
                            .child(i18n::tf(
                                cx,
                                "skeleton.double_press",
                                &[("count", &double_press)],
                            )),
                    )
                    .child(
                        button(&theme, "hand-tuning", i18n::t(cx, "skeleton.tuning"), false)
                            .on_click({
                                let panel = self.panel;
                                move |_, _, cx| {
                                    cx.default_global::<PanelControl>().spawns.push(
                                        PanelSpec::spawn(panel, |id, _, _| HandTuningTest::new(id))
                                            .sized(TUNING_SIZE, TUNING_PX)
                                            .titled("skeleton.tuning"),
                                    )
                                }
                            }),
                    ),
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

/// 骨格のみの手入力のチューニング画面（`hand-tuning`）。スライダー操作が即座に反映され、設定ファイルの `[input.hand]` に永続化される
/// （`Setting::Hand`）。`tune-pinch`（クリック距離）・`tune-little`（ショートカット距離）・`tune-grip`（グリップ角度）・
/// `tune-palm`（手のひら判定角度）・`tune-smoothing`（レーザーブレ抑制）・`tune-click`（クリック時の安定化）・`tune-hold`（クリック中のポインタ固定範囲）。`tune-reset` でデフォルトに復帰する。
pub struct HandTuningTest {}

impl HandTuningTest {
    pub fn new(_panel: PanelId) -> Self {
        Self {}
    }
}

impl Render for HandTuningTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let now = settings(cx).hand;
        // 値と範囲から、ラベル・スライダー・設定更新を 1 つの項目にまとめる。
        let item = |id: &'static str,
                    label: SharedString,
                    v: f32,
                    (lo, hi): (f32, f32),
                    set: fn(&mut HandTuning, f32)| {
            labeled(
                &theme,
                label,
                components::slider(&theme, id, (v - lo) / (hi - lo), move |t, _, cx| {
                    let mut h = settings(cx).hand;
                    set(&mut h, lo + t * (hi - lo));
                    ui::set(Setting::Hand(h), cx)
                })
                .length(600.0),
            )
        };
        let mm = |key: &str, v: f32| i18n::tf(cx, key, &[("mm", &format!("{v:.0}"))]);
        let deg = |key: &str, v: f32| i18n::tf(cx, key, &[("deg", &format!("{v:.0}"))]);
        let pct = |key: &str, v: f32| i18n::tf(cx, key, &[("pct", &percent(v))]);
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap(px(spacing::LG))
            .p(px(spacing::XL))
            .text_color(rgba(theme.neutral_text))
            .child(item(
                "tune-pinch",
                mm("skeleton.tune_pinch", now.pinch_mm),
                now.pinch_mm,
                HandTuning::PINCH_MM,
                |h, v| h.pinch_mm = v,
            ))
            .child(item(
                "tune-little",
                mm("skeleton.tune_little", now.little_mm),
                now.little_mm,
                HandTuning::PINCH_MM,
                |h, v| h.little_mm = v,
            ))
            .child(item(
                "tune-grip",
                deg("skeleton.tune_grip", now.grip_deg),
                now.grip_deg,
                HandTuning::GRIP_DEG,
                |h, v| h.grip_deg = v,
            ))
            .child(item(
                "tune-palm",
                deg("skeleton.tune_palm", now.palm_deg),
                now.palm_deg,
                HandTuning::PALM_DEG,
                |h, v| h.palm_deg = v,
            ))
            .child(item(
                "tune-smoothing",
                pct("skeleton.tune_smoothing", now.smoothing),
                now.smoothing,
                HandTuning::UNIT,
                |h, v| h.smoothing = v,
            ))
            .child(item(
                "tune-hold",
                deg("skeleton.tune_hold", now.click_hold_deg),
                now.click_hold_deg,
                HandTuning::CLICK_HOLD_DEG,
                |h, v| h.click_hold_deg = v,
            ))
            .child(item(
                "tune-click",
                pct("skeleton.tune_click", now.click_steady),
                now.click_steady,
                HandTuning::UNIT,
                |h, v| h.click_steady = v,
            ))
            .child(
                row().child(
                    button(
                        &theme,
                        "tune-reset",
                        i18n::t(cx, "common.default"),
                        now == HandTuning::default(),
                    )
                    .on_click(change(Setting::Hand(HandTuning::default()))),
                ),
            )
    }
}

/// 手のひらを `b` の中心に、HMD（`hmd`）視点の正射影で描画する。骨・関節点・関節ごとの方向ギズモを表示。
/// ハイライト対象の指（`lit`: 親指〜小指）は `colors.1` で描画し、他の骨は `colors.0` を使用する。
fn paint_hand(
    b: Bounds<Pixels>,
    s: &HandSkeleton,
    hmd: Pose,
    lit: [bool; 5],
    (bone, accent): (Hsla, Hsla),
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
    let lit_joint = |j: usize| finger_of(j).is_some_and(|f| lit[f]);
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
            let color = if lit_joint(j) { accent } else { bone };
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
                Bounds::new(
                    point(at.x - px(r), at.y - px(r)),
                    size(px(r * 2.0), px(r * 2.0)),
                ),
                if lit_joint(j) { accent } else { bone },
            )
            .corner_radii(px(r)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 指先を親指先端に近づけた指のみがピンチ状態になる。
    #[test]
    fn pinch_is_per_finger() {
        let mut s = HandSkeleton {
            hand: Hand::Right,
            joints: [Pose::IDENTITY; JOINT_COUNT],
            tracking: FingerTracking::Full,
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

    /// 骨格のみの手では、有効な操作に対応する指がハイライトされる（ピンチ中の指ではなく）。
    #[test]
    fn lit_fingers_follow_the_hand_actions() {
        let s = HandSkeleton {
            hand: Hand::Left,
            joints: [Pose::IDENTITY; JOINT_COUNT],
            tracking: FingerTracking::Full,
        };
        let mut c = kernel::subsystems::input::map(
            &kernel::vr::RawController {
                hand: Hand::Left,
                profile: kernel::subsystems::input::HAND_PROFILE.into(),
                model: String::new(),
                aim: Pose::IDENTITY,
                inputs: vec![("nyaw/palm_toward".into(), 1.0)],
            },
            None,
        );
        assert!(c.metrics.is_some());
        assert_eq!(lit_fingers(&s, Some(&c)), [false; 5]);
        c.grab = true;
        assert_eq!(lit_fingers(&s, Some(&c)), [false, false, true, true, true]);
        c = ControllerState {
            grab: false,
            shortcut: true,
            ..c
        };
        assert_eq!(lit_fingers(&s, Some(&c)), [true, false, false, false, true]);
        // 骨格データはあるがコントローラで操作している場合: ピンチ中の指（全関節が同一位置のため全指）。
        assert_eq!(lit_fingers(&s, None), [true; 5]);
    }
}
