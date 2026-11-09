//! LazyFollow: パネルを、ユーザーの向いている方向へゆるく追従させる。
//!
//! - 左右（水平の向き）だけを見る。上下の向きと頭の傾きは見ず、パネルは立てたまま頭の方（水平）を向く。HMD の高さには合わせる。
//! - ゆるい: 向きは不感帯（[`LazyFollow::dead_zone`]）の中なら動かさず、外れたら追いつくまで滑らかに回る。
//!   首を少し振っただけでパネルが逃げない（邪魔にならない）。
//! - 歩き回るとパネルも付いてくる。頭から水平に一定の距離（[`LazyFollow::distance`]）を保つ位置へ、遅れて追いつく。
//!
//! 状態を持つ純粋な計算なので、ランタイムなしでテストできる。パネルへの適用は `Overlay::set_lazy_follow`。

use crate::{Mat3, Pose, Quat, Vec3};

/// 向きの不感帯（ラジアン、25°）。ユーザーの向きとパネルの方位の差がこれを超えたら追いかけ始める。
const DEAD_ZONE: f32 = 0.436;
/// 追いかけを止める差（ラジアン、2°）。始める差より小さくして、境目で行き来しない。
const REST: f32 = 0.035;
/// 向きが差の 63% を埋めるまでの時間（秒）。
const TURN_TAU: f32 = 0.6;
/// 位置（距離・高さ）が差の 63% を埋めるまでの時間（秒）。
const MOVE_TAU: f32 = 0.3;

/// [`crate::Overlay::set_lazy_follow`] で付ける、追従の設定と今の状態。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LazyFollow {
    /// 頭から水平に保つ距離（m）。
    pub distance: f32,
    /// 頭の高さからの上下のずれ（m。上が正）。既定は 0（HMD と同じ高さ）。
    pub height: f32,
    /// 向きの不感帯（ラジアン）。
    pub dead_zone: f32,
    /// 向きの追いつきの時定数（秒）。大きいほどゆるい。
    pub turn_tau: f32,
    /// 位置の追いつきの時定数（秒）。
    pub move_tau: f32,
    state: Option<State>,
}

/// 追従の今の状態: パネルの位置と、頭から見たパネルの方位（水平角）。
#[derive(Debug, Clone, Copy, PartialEq)]
struct State {
    position: Vec3,
    yaw: f32,
    /// 方位を追いかけている最中（不感帯を出てから、ほぼ追いつくまで）。
    chasing: bool,
}

impl LazyFollow {
    /// 頭から `distance` m の距離で、HMD の高さに保つ。
    pub fn new(distance: f32) -> Self {
        Self {
            distance,
            height: 0.0,
            dead_zone: DEAD_ZONE,
            turn_tau: TURN_TAU,
            move_tau: MOVE_TAU,
            state: None,
        }
    }

    /// 頭の高さからのずれ（m）。
    pub fn at_height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// 向きの不感帯（ラジアン）。
    pub fn with_dead_zone(mut self, dead_zone: f32) -> Self {
        self.dead_zone = dead_zone;
        self
    }

    /// 向き・位置の追いつきの時定数（秒）。小さいほど速く追いつく（テストで短くするときなど）。
    pub fn with_time_constants(mut self, turn: f32, movement: f32) -> Self {
        (self.turn_tau, self.move_tau) = (turn, movement);
        self
    }

    /// 1 フレーム進めて、パネルの新しい姿勢を返す。最初の 1 回は追いかけず、頭の正面にそのまま置く。
    pub fn step(&mut self, head: &Pose, dt: f32) -> Pose {
        let look = bearing(head.forward());
        let (mut state, first) = match self.state {
            Some(s) => (s, false),
            None => {
                let yaw = look.unwrap_or(0.0);
                let position = self.target(head, yaw);
                (
                    State {
                        position,
                        yaw,
                        chasing: false,
                    },
                    true,
                )
            }
        };
        if !first && let Some(look) = look {
            let diff = wrap(look - state.yaw);
            if diff.abs() > self.dead_zone {
                state.chasing = true;
            } else if diff.abs() < REST {
                state.chasing = false;
            }
            if state.chasing {
                state.yaw = wrap(state.yaw + diff * blend(dt, self.turn_tau));
            }
        }
        if !first {
            let target = self.target(head, state.yaw);
            state.position = state.position.lerp(target, blend(dt, self.move_tau));
        }
        self.state = Some(state);
        // 頭の方を水平に向く（上下・傾きは見ない）。真上・真下に頭があるときだけ向きを保つ。
        let to_head = Vec3::new(
            head.position.x - state.position.x,
            0.0,
            head.position.z - state.position.z,
        );
        let z =
            to_head
                .try_normalize()
                .unwrap_or(Vec3::new(-state.yaw.sin(), 0.0, state.yaw.cos()));
        let x = Vec3::Y.cross(z);
        Pose::new(
            state.position,
            Quat::from_mat3(&Mat3::from_cols(x, Vec3::Y, z)),
        )
    }

    /// 方位 `yaw` に、頭から水平に `distance`、頭の高さ + `height` で置いた位置。
    fn target(&self, head: &Pose, yaw: f32) -> Vec3 {
        Vec3::new(
            head.position.x + yaw.sin() * self.distance,
            head.position.y + self.height,
            head.position.z - yaw.cos() * self.distance,
        )
    }

    /// パネルの今の方位（頭から見た水平角）。置く前は `None`。
    pub fn yaw(&self) -> Option<f32> {
        self.state.map(|s| s.yaw)
    }
}

/// 水平面（XZ）での向きの角。-Z（正面）が 0 で、+X へ回るほど増える。真上・真下を向いていれば `None`。
fn bearing(forward: Vec3) -> Option<f32> {
    let flat = Vec3::new(forward.x, 0.0, forward.z);
    (flat.length() > 1e-3).then(|| flat.x.atan2(-flat.z))
}

/// 角度を -π〜π に収める。
fn wrap(a: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    (a + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI
}

/// 時定数 `tau` の指数的な追いつきの、`dt` 秒での進み割合（0〜1）。
fn blend(dt: f32, tau: f32) -> f32 {
    1.0 - (-dt / tau.max(1e-3)).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(x: f32, y: f32, z: f32, yaw: f32) -> Pose {
        // yaw は +X 方向へ向く向きを正（`bearing` と同じ）にする。
        Pose::new(Vec3::new(x, y, z), Quat::from_rotation_y(-yaw))
    }

    fn settle(lf: &mut LazyFollow, h: &Pose) -> Pose {
        let mut p = lf.step(h, 0.011);
        for _ in 0..2000 {
            p = lf.step(h, 0.011);
        }
        p
    }

    #[test]
    fn starts_in_front_of_the_head_at_head_height_facing_it() {
        let mut lf = LazyFollow::new(1.5);
        let p = lf.step(&head(0.0, 1.6, 0.0, 0.0), 0.011);
        assert!(
            p.position.abs_diff_eq(Vec3::new(0.0, 1.6, -1.5), 1e-4),
            "{:?}",
            p.position
        );
        // 表面（+Z）が頭の方（+Z）を向く。
        assert!((p.orientation * Vec3::Z).abs_diff_eq(Vec3::Z, 1e-4));
    }

    #[test]
    fn small_head_turns_are_ignored_but_a_big_turn_is_followed() {
        let mut lf = LazyFollow::new(1.0);
        let start = lf.step(&head(0.0, 1.6, 0.0, 0.0), 0.011);
        // 不感帯の中（10°）: 動かない。
        let small = settle(&mut lf, &head(0.0, 1.6, 0.0, 10f32.to_radians()));
        assert!(
            small.position.abs_diff_eq(start.position, 1e-3),
            "{:?}",
            small.position
        );
        // 不感帯の外（90°）: 向いた方へ回って来る（追いつくと正面）。
        let big = settle(&mut lf, &head(0.0, 1.6, 0.0, 90f32.to_radians()));
        assert!(
            big.position.abs_diff_eq(Vec3::new(1.0, 1.6, 0.0), 5e-2),
            "{:?}",
            big.position
        );
    }

    #[test]
    fn follows_walking_at_a_constant_distance_and_height() {
        let mut lf = LazyFollow::new(1.2);
        lf.step(&head(0.0, 1.6, 0.0, 0.0), 0.011);
        // 歩いて、しゃがむ（高さに合わせる）。
        let h = head(3.0, 1.1, -2.0, 0.0);
        let p = settle(&mut lf, &h);
        let flat = Vec3::new(
            p.position.x - h.position.x,
            0.0,
            p.position.z - h.position.z,
        );
        assert!((flat.length() - 1.2).abs() < 1e-2, "{}", flat.length());
        assert!((p.position.y - 1.1).abs() < 1e-2, "高さ {}", p.position.y);
    }

    #[test]
    fn looking_up_or_tilting_the_head_does_not_move_or_tilt_the_panel() {
        let mut lf = LazyFollow::new(1.0);
        let flat = lf.step(&head(0.0, 1.6, 0.0, 0.0), 0.011);
        // 見上げて首も傾ける。
        let tilted = Pose::new(
            Vec3::new(0.0, 1.6, 0.0),
            Quat::from_rotation_x(0.8) * Quat::from_rotation_z(0.5),
        );
        let p = settle(&mut lf, &tilted);
        assert!(p.position.abs_diff_eq(flat.position, 1e-2));
        // パネルは立ったまま（上が +Y）。
        assert!((p.orientation * Vec3::Y).abs_diff_eq(Vec3::Y, 1e-4));
    }

    #[test]
    fn wraps_the_shortest_way_round() {
        assert!((wrap(3.5 * std::f32::consts::PI) + 0.5 * std::f32::consts::PI).abs() < 1e-4);
        let mut lf = LazyFollow::new(1.0);
        lf.step(&head(0.0, 0.0, 0.0, 170f32.to_radians()), 0.011);
        // 170° → -170° は 20° の差（不感帯の中）で、逆回りに 340° 回らない。
        let p = lf.step(&head(0.0, 0.0, 0.0, -170f32.to_radians()), 0.011);
        assert!(lf.yaw().unwrap().abs() > 2.9, "{:?}", lf.yaw());
        let _ = p;
    }
}
