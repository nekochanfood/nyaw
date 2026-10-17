//! 入力サブシステム（ドライバ）。VR サブシステムから来るデバイスごとの生入力を、
//! オーバーレイ層が理解する少数のアクションに変換する。コントローラ追加 = [`PROFILES`] に1行追加。
//! この層より上でプロファイル名を見てはいけない。

use vr::{Facing, Hand, HandSkeleton, Mat3, Pose, Quat, RawController, TIPS, Vec2, Vec3};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ClickButton {
    #[default]
    Left,
    Middle,
    Right,
}

/// オーバーレイ処理がどのコントローラからも必要とする状態。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControllerState {
    pub hand: Hand,
    /// レーザーの原点と向き（前方 = -Z）。
    pub aim: Pose,
    /// UI 要素のクリック・押下。
    pub select: bool,
    pub click_button: ClickButton,
    /// select の入力の強さ（0〜1。トリガーの引き具合）。押下の判定前の引き始めを知るため。
    pub trigger: f32,
    /// パネルを掴んで動かす。
    pub grab: bool,
    pub menu: bool,
    /// ショートカットボタン（左手の下のボタン。Touch は X、Index は左手の A）。何に使うかは画面が決める。右手では常に `false`。
    pub shortcut: bool,
    pub scroll: Vec2,
    /// 掴んでいるパネルの拡大縮小（-1..1、+ で大きく）。機種の割り当ては無く、[`vr::RESIZE_INPUT`] をそのまま読む。
    pub resize: f32,
    /// 顔に向けているのが手のひらか手の甲か。骨格だけの手（[`HAND_PROFILE`]）のときだけ分かる。
    pub facing: Option<Facing>,
    /// 骨格だけの手の判定に使った値（確認用）。コントローラでは `None`。
    pub metrics: Option<HandMetrics>,
}

/// アクションごとの入力名（OpenXR の語彙）。報告された候補のうち最も大きい値を採用する。
struct Profile {
    id: &'static str,
    select: &'static [&'static str],
    grab: &'static [&'static str],
    menu: &'static [&'static str],
    /// 左手のときだけ読む。
    shortcut: &'static [&'static str],
    scroll: Option<(&'static str, &'static str)>,
}

const SIMPLE: Profile = Profile {
    id: "khr/simple_controller",
    select: &["select/click"],
    grab: &[],
    menu: &["menu/click"],
    shortcut: &[],
    scroll: None,
};

const PROFILES: &[Profile] = &[
    SIMPLE,
    Profile {
        id: "valve/index_controller",
        // OpenVR では軸の種類の宣言がずれるドライバがあるので、トリガーのボタン（select/click）でも押せるようにする。
        select: &["trigger/value", "select/click"],
        grab: &["squeeze/value"],
        menu: &["b/click"],
        shortcut: &["a/click"],
        scroll: Some(("thumbstick/x", "thumbstick/y")),
    },
    Profile {
        id: "oculus/touch_controller",
        // OpenVR では軸の種類の宣言がずれるドライバがあるので、トリガーのボタン（select/click）でも押せるようにする。
        select: &["trigger/value", "select/click"],
        grab: &["squeeze/value"],
        menu: &["menu/click", "y/click", "b/click"],
        shortcut: &["x/click"],
        scroll: Some(("thumbstick/x", "thumbstick/y")),
    },
    // コントローラの無い手（スケルタル入力。[`hand_controller`] が作る）。親指と人差し指でつまむとクリック、握ると掴む。
    Profile {
        id: HAND_PROFILE,
        select: &["pinch_ext/value"],
        grab: &["grasp_ext/value"],
        menu: &[],
        // 左手の親指と小指でつまむ（顔に手のひらを向けている間だけ。二回でダッシュボード）。
        shortcut: &[LITTLE_PINCH_INPUT],
        scroll: None,
    },
    Profile {
        id: "htc/vive_controller",
        // OpenVR では軸の種類の宣言がずれるドライバがあるので、トリガーのボタン（select/click）でも押せるようにする。
        select: &["trigger/value", "select/click"],
        grab: &["squeeze/click"],
        menu: &["menu/click"],
        shortcut: &[],
        scroll: Some(("trackpad/x", "trackpad/y")),
    },
];

/// 骨格だけの手のプロファイル（OpenXR の XR_EXT_hand_interaction に揃える）。
pub const HAND_PROFILE: &str = "ext/hand_interaction_ext";
/// 骨格だけの手の、親指と小指のつまみの強さ（手のひらを顔に向けていなければ 0）。OpenXR に無いので nyaw の名前。
const LITTLE_PINCH_INPUT: &str = "nyaw/little_pinch";
/// 手のひらの向きによらない、親指と小指のつまみの強さ（[`HandMetrics::little`]。判定の確認用）。
const LITTLE_RAW_INPUT: &str = "nyaw/little_raw";
/// 骨格だけの手が顔へ手のひらを向けている度合い（[`vr::HandSkeleton::palm_toward`]。-1〜1）。
const PALM_TOWARD_INPUT: &str = "nyaw/palm_toward";

/// つまみの強さが 0〜1 に変わる距離の幅（m）と、握りの強さが 0〜1 に変わる角度の幅（ラジアン）。押下（[`PRESS`]）はこの幅の 6 割の所、
/// 離す（`RELEASE`）のは 4 割の所なので、離すのはつまみで押下の距離 + 8mm、握りで押下の角度 - 16°。
const PINCH_SPAN_M: f32 = 0.04;
const GRIP_SPAN_RAD: f32 = 80.0 * std::f32::consts::PI / 180.0;

/// 骨格だけの手の調整値（`config.toml` の `[hand]`、`kernel::Setting::Hand`）。どれも省略できる。
// ponytail: 距離は手の大きさによらない固定値。手の大きさで効き方が違うなら、手の長さで割った比にする。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct HandTuning {
    /// 親指と人差し指の先がこの距離（mm）まで近づくとクリック。離すのはこれ + 8mm。
    pub pinch_mm: f32,
    /// 親指と小指の先がこの距離（mm）まで近づくとショートカット（左手の X ボタン）。小指は親指に届きにくいので少し緩い。
    pub little_mm: f32,
    /// 中指・薬指・小指の曲がり（中手骨と、基節から指先への向きのなす角）の平均がこの角度（度）を越えると掴む。離すのはこれ - 16°。
    pub grip_deg: f32,
    /// 手のひらの向きと顔へのなす角がこれ（度）以内なら、手のひらを顔に向けているとみなす（ショートカットの条件）。
    pub palm_deg: f32,
    /// 手のレーザーの揺れの抑え（0〜1）。大きいほど止めたときに揺れないが、ゆっくり動かすときに遅れる（速く振れば付いてくる）。
    pub smoothing: f32,
    /// つまみかけてから離すまで（握っている間も）レーザーを鈍くする強さ（0〜1）。1 に近いほどクリックでカーソルが動かない。
    pub click_steady: f32,
    /// クリックしている（つまんでいる）間、押した瞬間のレーザーで止めておく範囲（度）。手がこれより大きく動けば、
    /// 止めるのをやめて付いていく（ドラッグ・スワイプ）。0 で止めない。
    pub click_hold_deg: f32,
}

impl Default for HandTuning {
    fn default() -> Self {
        Self {
            pinch_mm: 26.0,
            // 二回つまむので、離したことをはっきり分けるよう、しっかり付けるまで押下にしない（離すのは + 8mm）。
            little_mm: 16.0,
            grip_deg: 88.0,
            palm_deg: 70.0,
            smoothing: 0.6,
            click_steady: 0.85,
            click_hold_deg: 6.0,
        }
    }
}

impl HandTuning {
    /// 調整できる範囲（下限, 上限）。画面のスライダーと読み込みの両方で使う。
    pub const PINCH_MM: (f32, f32) = (10.0, 50.0);
    pub const GRIP_DEG: (f32, f32) = (50.0, 140.0);
    pub const PALM_DEG: (f32, f32) = (20.0, 90.0);
    pub const UNIT: (f32, f32) = (0.0, 1.0);
    pub const CLICK_HOLD_DEG: (f32, f32) = (0.0, 20.0);

    /// 範囲の外の値を範囲に収める（壊れた設定ファイルでも動くように）。
    pub fn clamped(self) -> Self {
        let c = |v: f32, (lo, hi): (f32, f32)| if v.is_finite() { v.clamp(lo, hi) } else { lo };
        Self {
            pinch_mm: c(self.pinch_mm, Self::PINCH_MM),
            little_mm: c(self.little_mm, Self::PINCH_MM),
            grip_deg: c(self.grip_deg, Self::GRIP_DEG),
            palm_deg: c(self.palm_deg, Self::PALM_DEG),
            smoothing: c(self.smoothing, Self::UNIT),
            click_steady: c(self.click_steady, Self::UNIT),
            click_hold_deg: c(self.click_hold_deg, Self::CLICK_HOLD_DEG),
        }
    }
}

/// 骨格だけの手の判定に使った値（確認用。[`ControllerState::metrics`]）。強さは [`PRESS`] で押下、`RELEASE` で離す。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HandMetrics {
    /// 親指と人差し指のつまみの強さ（クリック）。
    pub pinch: f32,
    /// 中指・薬指・小指の握りの強さ（掴む）。
    pub grip: f32,
    /// 親指と小指のつまみの強さ（手のひらの向きによらない。ショートカットはこれに手のひらの条件が付く）。
    pub little: f32,
    /// 手のひらを顔へ向けている度合い（なす角の cos）。
    pub palm_toward: f32,
}

/// 押下の距離 `press`（m）で [`PRESS`] になる、距離 `d` のつまみの強さ。
fn pinch_strength(d: f32, press: f32) -> f32 {
    ((press + PRESS * PINCH_SPAN_M - d) / PINCH_SPAN_M).clamp(0.0, 1.0)
}

/// 指（[`TIPS`] の添字）の曲がり（ラジアン。伸ばしきって 0、握りこぶしで 150° 前後）。
fn curl(skeleton: &HandSkeleton, finger: usize) -> f32 {
    let j = |i: usize| skeleton.joints[i].position;
    // 指ごとの関節は 中手骨, 基節, 中節, 末節, 先端 の順（先端から 4 つ前が中手骨）。
    let (metacarpal, proximal, tip) = (TIPS[finger] - 4, TIPS[finger] - 3, TIPS[finger]);
    (j(proximal) - j(metacarpal)).angle_between(j(tip) - j(proximal))
}

/// コントローラの無い手の骨格を、[`HAND_PROFILE`] の生入力にする（上位からは普通のコントローラに見える）。
/// レーザーの元は親指と人差し指の付け根（基節）の間で、向きは手の向き（手首から中指の付け根へ。[`HandSkeleton::grip`]）。
/// コントローラと同じく手首を曲げればレーザーが動き、指を曲げてもつまんでも動かない（手首と指の付け根だけで決まる）。
/// 揺れの抑えは [`HandFilter`]。
pub fn hand_controller(skeleton: &HandSkeleton, hmd: Pose, tuning: &HandTuning) -> RawController {
    let j = |i: usize| skeleton.joints[i].position;
    // 親指の基節（3）と人差し指の基節（7）の間。
    let origin = j(3).lerp(j(7), 0.5);
    let aim = Pose::new(origin, looking(skeleton.grip().forward()));
    let toward = skeleton.palm_toward(hmd.position);
    let little = pinch_strength(skeleton.pinch_distance(TIPS[4]), tuning.little_mm / 1e3);
    let facing_face = toward >= tuning.palm_deg.to_radians().cos();
    let curl = (2..5).map(|f| curl(skeleton, f)).sum::<f32>() / 3.0;
    let grip_none = tuning.grip_deg.to_radians() - PRESS * GRIP_SPAN_RAD;
    RawController {
        hand: skeleton.hand,
        profile: HAND_PROFILE.into(),
        aim,
        inputs: vec![
            (
                "pinch_ext/value".into(),
                pinch_strength(skeleton.pinch_distance(TIPS[1]), tuning.pinch_mm / 1e3),
            ),
            (
                "grasp_ext/value".into(),
                ((curl - grip_none) / GRIP_SPAN_RAD).clamp(0.0, 1.0),
            ),
            (
                LITTLE_PINCH_INPUT.into(),
                if facing_face { little } else { 0.0 },
            ),
            (LITTLE_RAW_INPUT.into(), little),
            (PALM_TOWARD_INPUT.into(), toward),
        ],
    }
}

/// 前方（-Z）を `forward` に向け、上を真上に寄せた向き。
fn looking(forward: Vec3) -> Quat {
    let back = (-forward).normalize_or(Vec3::Z);
    let x = Vec3::Y.cross(back).normalize_or(Vec3::X);
    Quat::from_mat3(&Mat3::from_cols(x, back.cross(x), back))
}

/// 揺れの抑えが最も弱い（`smoothing` = 0）・最も強い（1）ときの、止めている間の遮断周波数（Hz）。
const CUTOFF_LOOSE_HZ: f32 = 8.0;
const CUTOFF_TIGHT_HZ: f32 = 0.3;
/// 速く動かすほど遮断周波数を上げる係数（One Euro フィルタの β。位置は m/s、向きは rad/s あたりの Hz）。
const SPEED_POSITION: f32 = 4.0;
const SPEED_DIRECTION: f32 = 1.5;
/// クリックで鈍くしても、遮断周波数をこの割合より下げない（止まりきらないように）。
const CLICK_FLOOR: f32 = 0.02;

/// 手のレーザーの揺れを抑えるフィルタ（One Euro フィルタ。止めている間は強く抑え、速く動かすほど弱める）。手ごとに 1 つ。
/// ハンドトラッキングは関節が細かく揺れ、骨格の更新が表示より粗いランタイムでは段になって動くので、ここで滑らかにする。
#[derive(Debug, Clone, Copy, Default)]
pub struct HandFilter {
    /// 抑えた元と前方、その速さ（m/s, rad/s）。
    last: Option<(Vec3, Vec3, f32, f32)>,
    /// クリックしている間の止め方。
    hold: Hold,
}

/// クリックしている間のレーザー（[`HandTuning::click_hold_deg`]）。
#[derive(Debug, Clone, Copy, Default)]
enum Hold {
    /// クリックしていない。
    #[default]
    Off,
    /// 押した瞬間のレーザーで止めている。
    Held(Pose),
    /// 押したまま大きく動かしたので、離すまで付いていく。
    Moving,
}

/// 骨格の手に取り付けたもの（手首の時計）の、止めている間の遮断周波数（Hz）。
const GRIP_CUTOFF_HZ: f32 = 1.5;

/// 骨格の手に取り付けたもの（手首の時計）の姿勢の揺れを抑えるフィルタ（[`HandFilter`] と同じく、止めている間は強く、
/// 速く動かすほど弱く抑える）。手ごとに 1 つ。
#[derive(Debug, Clone, Copy, Default)]
pub struct PoseFilter {
    /// 抑えた姿勢と、その速さ（m/s, rad/s）。
    last: Option<(Pose, f32, f32)>,
}

impl PoseFilter {
    /// `pose` を抑えた姿勢。`dt` は前のフレームからの秒数。
    pub fn steady(&mut self, pose: Pose, dt: f32) -> Pose {
        let Some((p, v, w)) = self.last.filter(|_| dt > 0.0) else {
            self.last = Some((pose, 0.0, 0.0));
            return pose;
        };
        let k = alpha(1.0, dt);
        let v = v + (pose.position.distance(p.position) / dt - v) * k;
        let w = w + (pose.orientation.angle_between(p.orientation) / dt - w) * k;
        let steadied = Pose::new(
            p.position.lerp(
                pose.position,
                alpha(GRIP_CUTOFF_HZ + SPEED_POSITION * v, dt),
            ),
            p.orientation.slerp(
                pose.orientation,
                alpha(GRIP_CUTOFF_HZ + SPEED_DIRECTION * w, dt),
            ),
        );
        self.last = Some((steadied, v, w));
        steadied
    }
}

/// 遮断周波数 `cutoff` の 1 次ローパスで、経過 `dt` 秒に新しい値へ寄せる割合。
fn alpha(cutoff: f32, dt: f32) -> f32 {
    let tau = 1.0 / (std::f32::consts::TAU * cutoff);
    1.0 / (1.0 + tau / dt)
}

impl HandFilter {
    /// `raw` のレーザーを抑える。`prev` は前のフレームのこの手の状態（押している間は鈍くするため）、`dt` は前のフレームからの秒数。
    pub fn steady(
        &mut self,
        raw: &mut RawController,
        prev: Option<&ControllerState>,
        dt: f32,
        tuning: &HandTuning,
    ) {
        let (origin, forward) = (raw.aim.position, raw.aim.forward());
        let Some((o, f, v, w)) = self.last.filter(|_| dt > 0.0) else {
            self.last = Some((origin, forward, 0.0, 0.0));
            return;
        };
        // つまみかけ（押下の手前）〜押している・掴んでいる間は、その分だけ鈍くする。
        let pinch = raw.input("pinch_ext/value").unwrap_or(0.0);
        let held = prev.is_some_and(|p| p.select || p.grab);
        let progress = if held {
            1.0
        } else {
            (pinch / PRESS).clamp(0.0, 1.0)
        };
        let damp = (1.0 - tuning.click_steady * progress).max(CLICK_FLOOR);
        let base = CUTOFF_LOOSE_HZ * (CUTOFF_TIGHT_HZ / CUTOFF_LOOSE_HZ).powf(tuning.smoothing);
        // 速さは抑えた値からの離れ / dt を 1Hz で均したもの（One Euro は生の値の微分を使うが、こちらは遅れが溜まるほど速く追いつく）。
        let k = alpha(1.0, dt);
        let v = v + (origin.distance(o) / dt - v) * k;
        let w = w + (forward.angle_between(f) / dt - w) * k;
        let o = o.lerp(origin, alpha((base + SPEED_POSITION * v) * damp, dt));
        let f = f
            .lerp(forward, alpha((base + SPEED_DIRECTION * w) * damp, dt))
            .normalize_or(forward);
        self.last = Some((o, f, v, w));
        raw.aim = Pose::new(o, looking(f));
        // クリックしている間は押した瞬間のレーザーで止め、つまむ・離す指の動きでカーソルがずれないようにする。
        // 止める範囲を越えて動かしたら、ドラッグとして付いていく。
        if !prev.is_some_and(|p| p.select) {
            self.hold = Hold::Off;
            return;
        }
        let limit = tuning.click_hold_deg.to_radians();
        self.hold = match self.hold {
            Hold::Off if limit > 0.0 => Hold::Held(raw.aim),
            Hold::Held(at) if at.forward().angle_between(f) > limit => Hold::Moving,
            Hold::Off => Hold::Moving,
            other => other,
        };
        if let Hold::Held(at) = self.hold {
            raw.aim = at;
        }
    }
}

/// ヒステリシス: 押下は `PRESS` 超え、離すのは `RELEASE` 未満になってから。
// ponytail: 全アクション共通のしきい値。アクション毎に変えたくなったら Profile に持たせる。
pub const PRESS: f32 = 0.6;
const RELEASE: f32 = 0.4;

impl ControllerState {
    /// 押下になるまでの進み（0〜1）。ポインタ補正側は押下のしきい値を知らなくてよい。
    pub fn select_progress(&self) -> f32 {
        (self.trigger / PRESS).clamp(0.0, 1.0)
    }
}

fn held(v: f32, was: bool) -> bool {
    v > if was { RELEASE } else { PRESS }
}

fn strongest(raw: &RawController, names: &[&str]) -> f32 {
    names
        .iter()
        .filter_map(|n| raw.input(n))
        .fold(0.0, f32::max)
}

/// 未知のデバイスは KHR simple controller のバインドにフォールバックする。
fn profile(id: &str) -> &'static Profile {
    PROFILES.iter().find(|p| p.id == id).unwrap_or(&SIMPLE)
}

/// ボタン系の抽象アクション。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Select,
    Grab,
    Menu,
    /// 左手の X。何に使うかは画面が決める（今は wrist が二回押しでダッシュボードを開く）。
    Shortcut,
}

/// 逆引き: `profile` で `action` を起こす入力名。テストでの入力生成や操作ヒント表示用。
pub fn source(profile_id: &str, action: Action) -> Option<&'static str> {
    let p = profile(profile_id);
    match action {
        Action::Select => p.select,
        Action::Grab => p.grab,
        Action::Menu => p.menu,
        Action::Shortcut => p.shortcut,
    }
    .first()
    .copied()
}

/// 逆引き: `profile` でスクロールを起こす入力名（x, y）。スクロールできない機種は `None`。
pub fn scroll_source(profile_id: &str) -> Option<(&'static str, &'static str)> {
    profile(profile_id).scroll
}

/// `prev` は同じ手の前フレームの結果（ヒステリシス用。初回は `None`）。
pub fn map(raw: &RawController, prev: Option<&ControllerState>) -> ControllerState {
    let p = profile(&raw.profile);
    let was = |f: fn(&ControllerState) -> bool| prev.is_some_and(f);
    let trigger = strongest(raw, p.select);
    ControllerState {
        hand: raw.hand,
        aim: raw.aim,
        select: held(trigger, was(|c| c.select)),
        click_button: if p.id == "oculus/touch_controller" {
            let (right, middle) = match raw.hand {
                Hand::Left => ("x/touch", "y/touch"),
                Hand::Right => ("a/touch", "b/touch"),
            };
            if raw.input(right).unwrap_or(0.0) > 0.5 {
                ClickButton::Right
            } else if raw.input(middle).unwrap_or(0.0) > 0.5 {
                ClickButton::Middle
            } else {
                ClickButton::Left
            }
        } else {
            ClickButton::Left
        },
        trigger,
        grab: held(strongest(raw, p.grab), was(|c| c.grab)),
        menu: held(strongest(raw, p.menu), was(|c| c.menu)),
        shortcut: raw.hand == Hand::Left && held(strongest(raw, p.shortcut), was(|c| c.shortcut)),
        scroll: p.scroll.map_or(Vec2::ZERO, |(x, y)| {
            Vec2::new(raw.input(x).unwrap_or(0.0), raw.input(y).unwrap_or(0.0))
        }),
        resize: raw.input(vr::RESIZE_INPUT).unwrap_or(0.0).clamp(-1.0, 1.0),
        metrics: raw.input(PALM_TOWARD_INPUT).map(|palm_toward| HandMetrics {
            pinch: raw.input("pinch_ext/value").unwrap_or(0.0),
            grip: raw.input("grasp_ext/value").unwrap_or(0.0),
            little: raw.input(LITTLE_RAW_INPUT).unwrap_or(0.0),
            palm_toward,
        }),
        facing: raw
            .input(PALM_TOWARD_INPUT)
            .map(|t| if t >= 0.0 { Facing::Palm } else { Facing::Back }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(profile: &str, inputs: &[(&str, f32)]) -> RawController {
        RawController {
            hand: Hand::Right,
            profile: profile.into(),
            aim: Pose::IDENTITY,
            inputs: inputs.iter().map(|(n, v)| (n.to_string(), *v)).collect(),
        }
    }

    #[test]
    fn hysteresis_holds_between_thresholds() {
        let r = |v| raw("valve/index_controller", &[("trigger/value", v)]);
        let a = map(&r(0.5), None);
        assert!(!a.select); // 0.6 未満では押下にならない
        let b = map(&r(0.7), Some(&a));
        assert!(b.select);
        assert!(map(&r(0.5), Some(&b)).select); // 0.4 まで保持
        assert!(!map(&r(0.3), Some(&b)).select);
    }

    #[test]
    fn different_devices_same_action() {
        let index = map(
            &raw(
                "valve/index_controller",
                &[("trigger/value", 0.9), ("thumbstick/y", 1.0)],
            ),
            None,
        );
        let simple = map(
            &raw("khr/simple_controller", &[("select/click", 1.0)]),
            None,
        );
        assert!(index.select && simple.select);
        assert_eq!(index.scroll, Vec2::new(0.0, 1.0));
    }

    #[test]
    fn touch_menu_candidates_and_threshold() {
        let s = map(
            &raw(
                "oculus/touch_controller",
                &[("b/click", 1.0), ("squeeze/value", 0.3)],
            ),
            None,
        );
        assert!(s.menu);
        assert!(!s.grab);
    }

    #[test]
    fn source_roundtrips_through_map() {
        for id in PROFILES.iter().map(|p| p.id) {
            let name = source(id, Action::Select).unwrap();
            assert!(map(&raw(id, &[(name, 1.0)]), None).select, "{id}");
        }
        assert_eq!(source("khr/simple_controller", Action::Grab), None);
        let (x, y) = scroll_source("valve/index_controller").unwrap();
        let s = map(&raw("valve/index_controller", &[(x, 0.5), (y, -1.0)]), None).scroll;
        assert_eq!(s, Vec2::new(0.5, -1.0));
        assert_eq!(scroll_source("khr/simple_controller"), None);
    }

    /// X ボタンは左手だけ。右手の同じボタン（Index の A）は数えない。
    #[test]
    fn shortcut_is_left_hand_x() {
        let left = |r: RawController| RawController {
            hand: Hand::Left,
            ..r
        };
        let touch = raw("oculus/touch_controller", &[("x/click", 1.0)]);
        assert!(map(&left(touch), None).shortcut);
        let index = raw("valve/index_controller", &[("a/click", 1.0)]);
        assert!(!map(&index, None).shortcut);
        assert!(map(&left(index), None).shortcut);
        assert_eq!(source("htc/vive_controller", Action::Shortcut), None);
    }

    use std::f32::consts::PI;

    /// 大人の手の目安の長さで組んだ手。手首を `wrist` に、手のひらを下（-Y）・指を前方（-Z）に向け、`turn` で手首の周りに回す。
    /// `bend[f]` は指 f（[`TIPS`] の添字）の関節ごとの曲げ（ラジアン、手のひら側へ）。`pinch` の指先は親指の先に付ける。
    fn hand(
        side: Hand,
        wrist: Vec3,
        turn: Quat,
        bend: [f32; 5],
        pinch: Option<usize>,
    ) -> HandSkeleton {
        // 右手は親指が -X 側。左手は鏡写し。
        let m = if side == Hand::Right { 1.0 } else { -1.0 };
        let mut p = [Vec3::ZERO; vr::JOINT_COUNT];
        p[2] = Vec3::new(-0.02 * m, -0.01, -0.02);
        p[3] = Vec3::new(-0.04 * m, -0.015, -0.045);
        p[4] = Vec3::new(-0.05 * m, -0.02, -0.07);
        p[5] = Vec3::new(-0.055 * m, -0.02, -0.095);
        for (f, lane) in [(1, -0.02), (2, 0.0), (3, 0.018), (4, 0.034)] {
            let metacarpal = TIPS[f] - 4;
            p[metacarpal] = Vec3::new(lane * m * 0.5, 0.0, -0.01);
            let mut at = Vec3::new(lane * m, 0.0, -0.09);
            p[metacarpal + 1] = at;
            let mut dir = Vec3::NEG_Z;
            for (k, len) in [0.045, 0.028, 0.022].into_iter().enumerate() {
                dir = Quat::from_rotation_x(-bend[f]) * dir;
                at += dir * len;
                p[metacarpal + 2 + k] = at;
            }
        }
        if let Some(f) = pinch {
            p[TIPS[f]] = p[TIPS[0]] + Vec3::new(0.0, 0.005, 0.0);
        }
        p[vr::PALM] = p[12] * 0.5;
        HandSkeleton {
            hand: side,
            joints: p.map(|v| Pose::new(wrist + turn * v, turn)),
            tracking: vr::FingerTracking::Full,
        }
    }

    const OPEN: [f32; 5] = [0.0; 5];
    const RIGHT_WRIST: Vec3 = Vec3::new(0.25, 1.45, -0.25);

    fn head() -> Pose {
        Pose::new(Vec3::new(0.0, 1.6, 0.0), Quat::IDENTITY)
    }

    fn state(h: &HandSkeleton, head: Pose, prev: Option<&ControllerState>) -> ControllerState {
        map(&hand_controller(h, head, &HandTuning::default()), prev)
    }

    #[test]
    fn hand_pinch_clicks_and_closing_three_fingers_grabs() {
        let right = |bend, pinch| hand(Hand::Right, RIGHT_WRIST, Quat::IDENTITY, bend, pinch);
        let open = state(&right(OPEN, None), head(), None);
        assert!(!open.select && !open.grab);
        // レーザーは手の向き（指を前に伸ばした手なら前方 -Z）。元は親指と人差し指の付け根の間。
        assert!(
            open.aim.forward().abs_diff_eq(Vec3::NEG_Z, 1e-5),
            "{}",
            open.aim.forward()
        );
        let h = right(OPEN, None);
        assert_eq!(
            open.aim.position,
            h.joints[3].position.lerp(h.joints[7].position, 0.5)
        );
        // 手首ごと 30° 上へ曲げると、レーザーも 30° 上を向く。
        let up = Quat::from_rotation_x(30f32.to_radians());
        let bent = state(
            &hand(Hand::Right, RIGHT_WRIST, up, OPEN, None),
            head(),
            None,
        );
        assert!(bent.aim.forward().abs_diff_eq(up * Vec3::NEG_Z, 1e-5));

        let pinch = state(&right(OPEN, Some(1)), head(), Some(&open));
        assert!(pinch.select && !pinch.grab);

        // 中指・薬指・小指を軽く握る（関節ごとに約 50°）・握りこぶし（約 80°）で掴む。人差し指と親指は開いたまま。
        // 少し曲げた（約 35°。力を抜いた手）だけでは掴まない。
        for (deg, grab) in [(20.0f32, false), (35.0, false), (52.0, true), (80.0, true)] {
            let b = deg.to_radians();
            let s = state(&right([0.0, 0.0, b, b, b], None), head(), None);
            assert_eq!(s.grab, grab, "{deg}°");
            assert!(!s.select, "{deg}°");
        }
    }

    /// 指先でつまんでも、指を握ってもレーザーは動かない（手首と付け根で決まる）。
    #[test]
    fn hand_laser_ignores_pinching_and_curling() {
        let right = |bend, pinch| hand(Hand::Right, RIGHT_WRIST, Quat::IDENTITY, bend, pinch);
        let aim =
            |h: &HandSkeleton, head: Pose| hand_controller(h, head, &HandTuning::default()).aim;
        let base = aim(&right(OPEN, None), head());
        assert_eq!(aim(&right(OPEN, Some(1)), head()), base);
        assert_eq!(aim(&right([0.0, 0.0, 1.4, 1.4, 1.4], None), head()), base);
    }

    /// しきい値を設定で変えると、同じ手でも押下になる・ならないが変わる。判定に使った値は状態に残る。
    #[test]
    fn tuning_moves_the_thresholds() {
        // 親指と人差し指の先が 3cm 離れた手（既定の 2.6cm では押下にならない）。
        let mut h = hand(Hand::Right, RIGHT_WRIST, Quat::IDENTITY, OPEN, Some(1));
        h.joints[TIPS[1]].position = h.joints[TIPS[0]].position + Vec3::new(0.0, 0.03, 0.0);
        let with = |t: HandTuning| map(&hand_controller(&h, head(), &t), None);
        let d = with(HandTuning::default());
        assert!(!d.select);
        let m = d.metrics.unwrap();
        assert!(m.pinch > 0.4 && m.pinch < PRESS, "{m:?}");
        assert!(
            with(HandTuning {
                pinch_mm: 35.0,
                ..Default::default()
            })
            .select
        );
        // 握り: 関節ごとに 45° は既定（88°）で掴まず、しきい値を 70° に下げると掴む。
        let b = 45f32.to_radians();
        let h = hand(
            Hand::Right,
            RIGHT_WRIST,
            Quat::IDENTITY,
            [0.0, 0.0, b, b, b],
            None,
        );
        let grip = |t: HandTuning| map(&hand_controller(&h, head(), &t), None).grab;
        assert!(!grip(HandTuning::default()));
        assert!(grip(HandTuning {
            grip_deg: 70.0,
            ..Default::default()
        }));
        assert_eq!(
            HandTuning {
                pinch_mm: 500.0,
                smoothing: f32::NAN,
                ..Default::default()
            }
            .clamped(),
            HandTuning {
                pinch_mm: HandTuning::PINCH_MM.1,
                smoothing: 0.0,
                ..Default::default()
            }
        );
    }

    /// 90Hz で `frames` 回、`aim` を返す手を通したときのレーザーの前方。
    fn filtered(aims: impl IntoIterator<Item = (Vec3, f32)>, tuning: &HandTuning) -> Vec<Vec3> {
        let mut f = HandFilter::default();
        let mut prev = None;
        aims.into_iter()
            .map(|(dir, pinch)| {
                let mut raw = RawController {
                    hand: Hand::Right,
                    profile: HAND_PROFILE.into(),
                    aim: Pose::new(Vec3::ZERO, looking(dir)),
                    inputs: vec![("pinch_ext/value".into(), pinch)],
                };
                f.steady(&mut raw, prev.as_ref(), 1.0 / 90.0, tuning);
                prev = Some(map(&raw, prev.as_ref()));
                raw.aim.forward()
            })
            .collect()
    }

    /// クリックしている間は、手が揺れても押した瞬間のレーザーのまま。止める範囲を越えて動かせば付いていき、離すと戻る。
    #[test]
    fn clicking_holds_the_laser_until_the_hand_moves_away() {
        let t = HandTuning::default();
        let dir = |deg: f32| Quat::from_rotation_y(deg.to_radians()) * Vec3::NEG_Z;
        // つまんだまま ±2° 揺れる（クリックの指の動き）。
        let mut aims = vec![(dir(0.0), 1.0); 10];
        aims.extend((0..60).map(|i| (dir(if i % 2 == 0 { 2.0 } else { -2.0 }), 1.0)));
        let out = filtered(aims.clone(), &t);
        let held = out[10];
        assert!(
            out[10..].iter().all(|f| f.abs_diff_eq(held, 1e-6)),
            "止まっている"
        );
        // つまんだまま 30° 動かすと付いていく（ドラッグ）。
        aims.extend((0..90).map(|_| (dir(30.0), 1.0)));
        let out = filtered(aims.clone(), &t);
        assert!(out.last().unwrap().angle_between(dir(30.0)).to_degrees() < 3.0);
        // 0° にすると止めない。
        let loose = HandTuning {
            click_hold_deg: 0.0,
            ..t
        };
        let out = filtered(aims[..70].to_vec(), &loose);
        assert!(!out[10..].iter().all(|f| f.abs_diff_eq(out[10], 1e-6)));
    }

    /// 止めた手首の時計の細かな揺れは抑え、大きく動かせば付いてくる。
    #[test]
    fn pose_filter_damps_jitter_but_follows_moves() {
        let mut f = PoseFilter::default();
        let at = |x: f32| Pose::new(Vec3::new(x, 1.0, 0.0), Quat::IDENTITY);
        let mut out = Vec::new();
        for i in 0..180 {
            // 2mm の揺れ。
            out.push(f.steady(at(if i % 2 == 0 { 0.001 } else { -0.001 }), 1.0 / 90.0));
        }
        let shake = out[90..]
            .windows(2)
            .map(|w| w[0].position.distance(w[1].position))
            .fold(0.0f32, f32::max);
        assert!(shake < 0.0005, "{shake}");
        // 20cm 動かして 0.3 秒置くと、1cm 以内まで追いつく。
        let mut last = at(0.0);
        for _ in 0..27 {
            last = f.steady(at(0.2), 1.0 / 90.0);
        }
        assert!(
            last.position.distance(at(0.2).position) < 0.01,
            "{}",
            last.position
        );
    }

    /// 止めた手の細かな揺れは抑え、つまみかけるとさらに鈍くなり、速く振れば付いてくる。
    #[test]
    fn hand_filter_damps_jitter_and_clicks_but_follows_sweeps() {
        let t = HandTuning::default();
        // 1° の揺れ（フレームごとに左右へ）。
        let jitter = |pinch| {
            (0..180).map(move |i| {
                let a = if i % 2 == 0 { 0.5f32 } else { -0.5 }.to_radians();
                (Quat::from_rotation_y(a) * Vec3::NEG_Z, pinch)
            })
        };
        let spread = |v: &[Vec3]| {
            v[90..]
                .windows(2)
                .map(|w| w[0].angle_between(w[1]))
                .fold(0.0f32, f32::max)
                .to_degrees()
        };
        let open = spread(&filtered(jitter(0.0), &t));
        let pinching = spread(&filtered(jitter(0.5), &t));
        assert!(open < 0.3, "{open}°");
        assert!(pinching < open * 0.5, "{pinching}° vs {open}°");
        // 0.5 秒で 40° 振る: 振り終わって 0.2 秒後には 3° 以内まで追いつく。
        let sweep = (0..63).map(|i| {
            let a = (i.min(45) as f32 / 45.0 * 40.0).to_radians();
            (Quat::from_rotation_y(a) * Vec3::NEG_Z, 0.0)
        });
        let end = *filtered(sweep, &t).last().unwrap();
        let target = Quat::from_rotation_y(40f32.to_radians()) * Vec3::NEG_Z;
        assert!(end.angle_between(target).to_degrees() < 3.0);
    }

    /// 左手の親指と小指のつまみは、手のひらを顔に向けている間だけショートカットになる。向きは状態に残る。
    #[test]
    fn little_pinch_is_shortcut_only_with_palm_toward_face() {
        let wrist = Vec3::new(-0.15, 1.3, -0.3);
        // 手首を 180° ひねって手のひらを上（顔の側）へ。
        let up = Quat::from_rotation_z(PI);
        let left = |turn, pinch| hand(Hand::Left, wrist, turn, OPEN, pinch);
        let s = state(&left(up, Some(4)), head(), None);
        assert_eq!(s.facing, Some(Facing::Palm));
        assert!(s.shortcut && !s.select);
        // 二回目を数えられるよう、付けてから 2.6cm 離せば離したことになる（1.6cm で押下、2.4cm で離す）。
        let mut apart = left(up, Some(4));
        apart.joints[TIPS[4]].position =
            apart.joints[TIPS[0]].position + Vec3::new(0.0, 0.026, 0.0);
        assert!(!state(&apart, head(), Some(&s)).shortcut);
        // 2cm まで離しただけなら、まだ付いている。
        apart.joints[TIPS[4]].position = apart.joints[TIPS[0]].position + Vec3::new(0.0, 0.02, 0.0);
        assert!(state(&apart, head(), Some(&s)).shortcut);
        // 手の甲を向けている・つまんでいない間は数えない。
        let s = state(&left(Quat::IDENTITY, Some(4)), head(), None);
        assert_eq!(s.facing, Some(Facing::Back));
        assert!(!s.shortcut);
        assert!(!state(&left(up, None), head(), None).shortcut);
        // 右手では数えない。コントローラでは向きが分からない。
        let right = hand(Hand::Right, RIGHT_WRIST, up, OPEN, Some(4));
        assert!(!state(&right, head(), None).shortcut);
        assert_eq!(map(&raw("valve/index_controller", &[]), None).facing, None);
    }

    #[test]
    fn unknown_device_falls_back_to_simple() {
        assert!(map(&raw("acme/mystery", &[("select/click", 1.0)]), None).select);
    }

    /// トリガーの軸が 0 のままでも、ボタンのビットが立っていれば押せる。
    #[test]
    fn trigger_button_selects_when_axis_is_silent() {
        let r = raw(
            "oculus/touch_controller",
            &[("trigger/value", 0.0), ("select/click", 1.0)],
        );
        assert!(map(&r, None).select);
    }

    #[test]
    fn touch_click_modifiers_are_scoped_to_their_hand() {
        for (hand, own_right, own_middle, other_right, other_middle) in [
            (Hand::Left, "x/touch", "y/touch", "a/touch", "b/touch"),
            (Hand::Right, "a/touch", "b/touch", "x/touch", "y/touch"),
        ] {
            let mut r = raw(
                "oculus/touch_controller",
                &[(other_right, 1.0), (other_middle, 1.0)],
            );
            r.hand = hand;
            assert_eq!(map(&r, None).click_button, ClickButton::Left);
            r.inputs.push((own_middle.into(), 1.0));
            assert_eq!(map(&r, None).click_button, ClickButton::Middle);
            r.inputs.push((own_right.into(), 1.0));
            assert_eq!(map(&r, None).click_button, ClickButton::Right);
        }
    }
}
