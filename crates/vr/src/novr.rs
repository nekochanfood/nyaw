//! novr バックエンド: VR ランタイムを使わず、デスクトップのウィンドウで VR を模す（エミュレーター）。
//! ほかのランタイムと同じく、パネルの描画はテクスチャとして受け取り、固定した頭（原点・前方 -Z）
//! から見た絵をウィンドウに合成する。重なりは VR と同じく描画順（奥行きではない）。背景は透過がわかるよう灰色の市松模様。
//!
//! 入力は片手のコントローラとして模す:
//! - 右手: レイは目からマウスの方向（レーザーは視線と重なるので見えず、カーソルだけが見える）。
//!   左クリック = トリガー、右クリック = グリップ（掴む）、矢印キー = スティック、ホイール = スティックの縦。
//! - 左手: 手元で下を向いていてパネルを指さない。WASD = スティック。
//!
//! デバイスは実在しないので、取り付けたパネルは追従しない（[`Frame::emulated`]）。
//! testkit 用に、ウィンドウを出さない [`Emulator::headless`] と、入力の注入・スクリーンショットを公開する。

use minifb::{Key, MouseButton, MouseMode, Window, WindowOptions};

use crate::{
    Frame, Hand, OverlayId, Pose, Quat, RawController, Runtime, Vec2, Vec3, VrBackend, VrError,
};

const TITLE: &str = "nyaw (novr)";
const SIZE: (u32, u32) = (1280, 720);
/// 縦の視野角（ラジアン、70°）。
const FOV_Y: f32 = 1.221_730_5;
/// これより目に近いものは描かない（m）。面が目を通るクアッド（視線に沿ったレーザー）は真横から見えるので描かない。
const NEAR: f32 = 0.01;
/// 市松模様の 1 マス（px）と、無彩色の 2 色。
const CHECKER: u32 = 16;
const CHECKER_GRAY: [u32; 2] = [0x6b6b6b, 0x8a8a8a];
/// ウィンドウの更新の上限。VR ランタイムの vsync と同じく poll はここで待つ。
const FPS: usize = 90;
/// ホイール 1 刻みで右手のスティックを倒す量（1 フレームだけ）。オーバーレイはスティックの値をそのまま行数として送るので、
/// 1 刻み = 3 行（一般的なホイールの既定）になる。
// ponytail: 刻みの大きさ（Windows は 120 単位、ほかは OS ごと）は見ず向きだけ。滑らかなスクロールのホイールでは速すぎるかもしれない。
const WHEEL: f32 = 3.0;
/// 模すコントローラの機種（トリガー・グリップ・スティックがそろっている）。
const PROFILE: &str = "valve/index_controller";
/// 左手の置き場所（頭の少し下・左）。下を向けてパネルを指さないようにする。
const LEFT_HAND: Pose = Pose::new(
    Vec3::new(-0.2, -0.3, 0.0),
    Quat::from_xyzw(-0.707_106_77, 0.0, 0.0, 0.707_106_77),
);

/// 右手のボタン。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// 左クリック。
    Trigger,
    /// 右クリック。
    Grab,
}

#[derive(Debug)]
struct Quad {
    pose: Pose,
    width_m: f32,
    height_m: Option<f32>,
    visible: bool,
    /// 不透明度（0〜1）。テクスチャのアルファに掛ける。
    alpha: f32,
    sort: u32,
    /// 幅, 高さ, RGBA。
    texture: Option<(u32, u32, Vec<u8>)>,
}

/// novr のエミュレーター。本番は [`crate::connect`] がウィンドウ付きで作り、testkit は [`Emulator::headless`] で作る。
pub struct Emulator {
    /// `None` かつ `!headless` なら最初の poll で開く。
    window: Option<Window>,
    headless: bool,
    size: (u32, u32),
    /// 合成した絵（0RGB）。
    pixels: Vec<u32>,
    /// 背景の市松模様（`size` の大きさ。変わったら作り直す）。
    background: Vec<u32>,
    dirty: bool,
    quads: Vec<Quad>,
    /// マウスの位置（ウィンドウのピクセル、左上原点）。ウィンドウの外なら `None`（右手が見えない）。
    pointer: Option<Vec2>,
    trigger: bool,
    grab: bool,
    /// 左手・右手のスティック。
    sticks: [Vec2; 2],
    quit: bool,
}

impl Emulator {
    /// ウィンドウ付き。ウィンドウは最初の poll で開き、閉じると終了を求める。入力はウィンドウから受ける。
    pub(crate) fn windowed() -> Self {
        Self::new(SIZE, false)
    }

    /// ウィンドウを出さない（testkit 用）。入力は [`Emulator::point`] などで注入し、絵は [`Emulator::screenshot`] で見る。
    pub fn headless(width: u32, height: u32) -> Self {
        Self::new((width, height), true)
    }

    fn new(size: (u32, u32), headless: bool) -> Self {
        Self {
            window: None,
            headless,
            size,
            pixels: Vec::new(),
            background: Vec::new(),
            dirty: true,
            quads: Vec::new(),
            pointer: None,
            trigger: false,
            grab: false,
            sticks: [Vec2::ZERO; 2],
            quit: false,
        }
    }

    /// 画面の大きさ（px）。
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// マウスを画面上の点へ動かす（`None` でウィンドウの外へ出す）。ウィンドウ付きではウィンドウの入力で上書きされる。
    pub fn point(&mut self, at: Option<Vec2>) {
        self.pointer = at;
    }

    pub fn set_button(&mut self, button: Button, down: bool) {
        match button {
            Button::Trigger => self.trigger = down,
            Button::Grab => self.grab = down,
        }
    }

    /// スティックの倒し具合（-1..1、上が +Y）。左手は WASD、右手は矢印キーに当たる。
    pub fn set_stick(&mut self, hand: Hand, value: Vec2) {
        self.sticks[hand as usize] = value;
    }

    /// 頭の座標系（= トラッキング空間）の点が映る画面上の点。目の後ろなら `None`。
    pub fn project(&self, p: Vec3) -> Option<Vec2> {
        if p.z > -NEAR {
            return None;
        }
        let (k, b) = self.lens();
        Some((Vec2::new(p.x, p.y) / -p.z - b) / k)
    }

    /// 画面上の点を通る、目からの視線（z = -1 に正規化）。[`Emulator::project`] の逆。
    fn ray(&self, at: Vec2) -> Vec3 {
        let (k, b) = self.lens();
        (at * k + b).extend(-1.0)
    }

    /// 画面の点 `at` を通る視線の x, y（z = -1）が `at * k + b` になる係数。
    fn lens(&self) -> (Vec2, Vec2) {
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        let t = (FOV_Y / 2.0).tan();
        let k = Vec2::new(2.0 * t / h, -2.0 * t / h);
        (k, Vec2::new(-t * w / h, t))
    }

    /// 今の画面（幅, 高さ, RGBA）。ユーザーに見えている絵。
    pub fn screenshot(&mut self) -> (u32, u32, Vec<u8>) {
        self.render();
        let rgba = self
            .pixels
            .iter()
            .flat_map(|p| {
                let [_, r, g, b] = p.to_be_bytes();
                [r, g, b, 0xff]
            })
            .collect();
        (self.size.0, self.size.1, rgba)
    }

    fn controllers(&self) -> Vec<RawController> {
        let raw = |hand, aim, trigger: bool, grab: bool, stick: Vec2| RawController {
            hand,
            profile: PROFILE.to_owned(),
            aim,
            inputs: vec![
                ("trigger/value".to_owned(), f32::from(u8::from(trigger))),
                ("squeeze/value".to_owned(), f32::from(u8::from(grab))),
                ("thumbstick/x".to_owned(), stick.x),
                ("thumbstick/y".to_owned(), stick.y),
            ],
        };
        let mut out = vec![raw(Hand::Left, LEFT_HAND, false, false, self.sticks[0])];
        if let Some(at) = self.pointer {
            let dir = self.ray(at).normalize();
            let aim = Pose::new(Vec3::ZERO, Quat::from_rotation_arc(Vec3::NEG_Z, dir));
            out.push(raw(
                Hand::Right,
                aim,
                self.trigger,
                self.grab,
                self.sticks[1],
            ));
        }
        out
    }

    /// ウィンドウを（無ければ開いて）入力を読む。
    fn read_window(&mut self) {
        if self.window.is_none() {
            let opts = WindowOptions {
                resize: true,
                ..Default::default()
            };
            match Window::new(TITLE, SIZE.0 as usize, SIZE.1 as usize, opts) {
                Ok(mut w) => {
                    w.set_target_fps(FPS);
                    self.window = Some(w);
                }
                Err(e) => {
                    logger::error!("エミュレーターのウィンドウを開けない", error = e);
                    self.quit = true;
                    return;
                }
            }
        }
        let Some(w) = &self.window else { return };
        if !w.is_open() {
            self.quit = true;
            return;
        }
        let (ww, wh) = w.get_size();
        if ww > 0 && wh > 0 && (ww as u32, wh as u32) != self.size {
            self.size = (ww as u32, wh as u32);
            self.dirty = true;
        }
        // 押したままウィンドウの外へ出ても、掴んだパネルやドラッグを放さないよう縁に留める。
        let mode = if self.trigger || self.grab {
            MouseMode::Clamp
        } else {
            MouseMode::Discard
        };
        self.pointer = w.get_mouse_pos(mode).map(|(x, y)| Vec2::new(x, y));
        self.trigger = w.get_mouse_down(MouseButton::Left);
        self.grab = w.get_mouse_down(MouseButton::Right);
        let stick = |[left, right, down, up]: [Key; 4]| {
            let axis = |neg, pos| {
                f32::from(u8::from(w.is_key_down(pos))) - f32::from(u8::from(w.is_key_down(neg)))
            };
            Vec2::new(axis(left, right), axis(down, up))
        };
        self.sticks = [
            stick([Key::A, Key::D, Key::S, Key::W]),
            stick([Key::Left, Key::Right, Key::Down, Key::Up]),
        ];
        // ホイールは右手のスティックの縦。回したフレームだけ倒す（量は向きだけ見る。OS ごとに単位が違う）。
        if let Some((_, y)) = w.get_scroll_wheel()
            && y != 0.0
        {
            self.sticks[1].y = y.signum() * WHEEL;
        }
    }

    /// 変化があれば合成し直す。
    fn render(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let (w, h) = self.size;
        if self.background.len() != (w * h) as usize {
            self.background = (0..w * h)
                .map(|i| {
                    let (x, y) = (i % w, i / w);
                    CHECKER_GRAY[((x / CHECKER + y / CHECKER) % 2) as usize]
                })
                .collect();
        }
        self.pixels.clear();
        self.pixels.extend_from_slice(&self.background);
        let mut order: Vec<usize> = (0..self.quads.len())
            .filter(|&i| {
                let q = &self.quads[i];
                q.visible && q.alpha > 0.0 && q.texture.is_some()
            })
            .collect();
        // 安定ソートなので、同じ順位なら作った順に重ねる。
        order.sort_by_key(|&i| self.quads[i].sort);
        for i in order {
            self.draw(i);
        }
    }

    /// クアッド `i` を画面に重ねる（ピクセルごとに視線とクアッドの交点のテクセルを取る）。
    fn draw(&mut self, i: usize) {
        let q = &self.quads[i];
        let Some((tw, th, tex)) = &q.texture else {
            return;
        };
        let (tw, th) = (*tw, *th);
        let size = Vec2::new(
            q.width_m,
            q.height_m.unwrap_or(q.width_m * th as f32 / tw as f32),
        );
        let (ux, uy, n) = (
            q.pose.orientation * Vec3::X,
            q.pose.orientation * Vec3::Y,
            q.pose.orientation * Vec3::Z,
        );
        let c = q.pose.position;
        let alpha = (q.alpha * 255.0).round() as u32;
        let nc = n.dot(c);
        if nc.abs() < NEAR || tw == 0 || th == 0 {
            return;
        }
        // 4 隅が写る範囲だけを走査する。目の後ろに隅があれば全面。
        let (w, h) = self.size;
        let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(sx, sy)| self.project(c + ux * size.x / 2.0 * sx + uy * size.y / 2.0 * sy));
        let (x0, y0, x1, y1) = match corners {
            [Some(a), Some(b), Some(cc), Some(d)] => {
                let lo = a.min(b).min(cc).min(d).max(Vec2::ZERO);
                let hi = a.max(b).max(cc).max(d).min(Vec2::new(w as f32, h as f32));
                (
                    lo.x as u32,
                    lo.y as u32,
                    hi.x.ceil() as u32,
                    hi.y.ceil() as u32,
                )
            }
            _ => (0, 0, w, h),
        };
        // 視線 d = (x*kx + bx, y*ky + by, -1) とクアッドの交点は t = nc / (n·d)。テクスチャ座標は (ux·d) t、(uy·d) t から
        // 出るので、3 つの内積を行ごとに x について足し込めば、ピクセルごとの計算は割り算 1 回で済む。
        let (k, b) = self.lens();
        let dot = |a: Vec3| (a.x * k.x, a.x * b.x - a.z, a.y * k.y, a.y * b.y);
        let (n_x, n_c, n_y, n_b) = dot(n);
        let (u_x, u_c, u_y, u_b) = dot(ux);
        let (v_x, v_c, v_y, v_b) = dot(uy);
        let (cu, cv) = (c.dot(ux), c.dot(uy));
        let (tw_f, th_f) = (tw as f32, th as f32);
        for y in y0..y1 {
            let py = y as f32 + 0.5;
            let row = |x_k: f32, c0: f32, y_k: f32, y_b: f32| (x_k, c0 + y_k * py + y_b);
            let (n_dx, mut dn) = row(n_x, n_c, n_y, n_b);
            let (u_dx, mut du) = row(u_x, u_c, u_y, u_b);
            let (v_dx, mut dv) = row(v_x, v_c, v_y, v_b);
            let px0 = x0 as f32 + 0.5;
            dn += n_dx * px0;
            du += u_dx * px0;
            dv += v_dx * px0;
            let line = (y * w) as usize;
            for x in x0..x1 {
                let (pn, pu, pv) = (dn, du, dv);
                dn += n_dx;
                du += u_dx;
                dv += v_dx;
                if pn.abs() < 1e-9 {
                    continue;
                }
                let t = nc / pn;
                if t < NEAR {
                    continue;
                }
                let u = (pu * t - cu) / size.x + 0.5;
                let v = 0.5 - (pv * t - cv) / size.y;
                if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                    continue;
                }
                let tx = ((u * tw_f) as u32).min(tw - 1);
                let ty = ((v * th_f) as u32).min(th - 1);
                let s = ((ty * tw + tx) * 4) as usize;
                let a = u32::from(tex[s + 3]) * alpha / 255;
                if a == 0 {
                    continue;
                }
                let [r, g, b] = [tex[s], tex[s + 1], tex[s + 2]].map(u32::from);
                let dst = &mut self.pixels[line + x as usize];
                *dst = if a == 255 {
                    (r << 16) | (g << 8) | b
                } else {
                    let [_, dr, dg, db] = dst.to_be_bytes().map(u32::from);
                    let mix = |s: u32, d: u32| (s * a + d * (255 - a)) / 255;
                    (mix(r, dr) << 16) | (mix(g, dg) << 8) | mix(b, db)
                };
            }
        }
    }

    fn quad(&mut self, id: OverlayId) -> &mut Quad {
        self.dirty = true;
        &mut self.quads[id.0 as usize]
    }
}

impl VrBackend for Emulator {
    fn runtime(&self) -> Runtime {
        Runtime::NoVr
    }

    /// ウィンドウ付きでは、前のフレームの絵を出して入力を読み、[`FPS`] の周期まで待つ。
    fn poll(&mut self) -> Frame {
        if !self.headless {
            self.read_window();
            self.render();
            let (w, h) = (self.size.0 as usize, self.size.1 as usize);
            if let Some(win) = &mut self.window {
                let shown = if self.pixels.len() == w * h {
                    win.update_with_buffer(&self.pixels, w, h)
                } else {
                    // 大きさが変わった直後で、まだ合成していない。
                    win.update();
                    Ok(())
                };
                if let Err(e) = shown {
                    logger::warn!("エミュレーターの画面を更新できない", error = e);
                }
            }
        }
        Frame {
            hmd: Pose::IDENTITY,
            controllers: self.controllers(),
            quit: self.quit,
            emulated: true,
            ..Frame::default()
        }
    }

    fn create_overlay(&mut self, _key: &str, width_m: f32) -> Result<OverlayId, VrError> {
        self.quads.push(Quad {
            pose: Pose::IDENTITY,
            width_m,
            height_m: None,
            visible: true,
            alpha: 1.0,
            sort: 0,
            texture: None,
        });
        Ok(OverlayId(self.quads.len() as u64 - 1))
    }

    fn set_overlay_pose(&mut self, id: OverlayId, pose: Pose) {
        self.quad(id).pose = pose;
    }

    fn set_overlay_width(&mut self, id: OverlayId, width_m: f32) {
        self.quad(id).width_m = width_m;
    }

    fn set_overlay_height(&mut self, id: OverlayId, height_m: f32) {
        self.quad(id).height_m = Some(height_m);
    }

    fn set_overlay_visible(&mut self, id: OverlayId, visible: bool) {
        self.quad(id).visible = visible;
    }

    fn set_overlay_alpha(&mut self, id: OverlayId, alpha: f32) -> bool {
        self.quad(id).alpha = alpha.clamp(0.0, 1.0);
        true
    }

    fn set_overlay_sort(&mut self, id: OverlayId, order: u32) {
        self.quad(id).sort = order;
    }

    fn submit_overlay_pixels(&mut self, id: OverlayId, width: u32, height: u32, rgba: &[u8]) {
        self.quad(id).texture = Some((width, height, rgba.to_vec()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emulator_with_red_panel() -> (Emulator, OverlayId) {
        let mut e = Emulator::headless(200, 100);
        let id = e.create_overlay("t", 1.0).unwrap();
        e.set_overlay_pose(id, Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY));
        e.submit_overlay_pixels(id, 2, 1, &[255, 0, 0, 255, 0, 0, 255, 128]);
        (e, id)
    }

    fn pixel(shot: &(u32, u32, Vec<u8>), at: Vec2) -> [u8; 4] {
        let i = ((at.y as u32 * shot.0 + at.x as u32) * 4) as usize;
        shot.2[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn project_and_ray_are_inverse() {
        let e = Emulator::headless(200, 100);
        let p = e.project(Vec3::new(0.3, -0.2, -1.5)).unwrap();
        let r = e.ray(p) * 1.5;
        assert!(r.abs_diff_eq(Vec3::new(0.3, -0.2, -1.5), 1e-4));
        assert_eq!(
            e.project(Vec3::new(0.0, 0.0, -1.0)),
            Some(Vec2::new(100.0, 50.0))
        );
        assert_eq!(e.project(Vec3::new(0.0, 0.0, 1.0)), None);
    }

    #[test]
    fn panel_is_drawn_over_gray_checker_with_alpha() {
        let (mut e, _) = emulator_with_red_panel();
        let shot = e.screenshot();
        assert_eq!((shot.0, shot.1), (200, 100));
        // 左半分は不透明な赤、右半分は半透明の青が市松模様に重なる。
        let left = e.project(Vec3::new(-0.25, 0.0, -1.0)).unwrap();
        assert_eq!(pixel(&shot, left), [255, 0, 0, 255]);
        let right = pixel(&shot, e.project(Vec3::new(0.25, 0.0, -1.0)).unwrap());
        assert_eq!(right[0], right[1], "灰色に青が混ざる");
        assert!(right[2] > right[0]);
        // パネルの外は無彩色の市松模様。
        let corner = pixel(&shot, Vec2::new(0.0, 0.0));
        assert!(corner[0] == corner[1] && corner[1] == corner[2]);
        let next = pixel(&shot, Vec2::new(CHECKER as f32, 0.0));
        assert_ne!(corner, next);
    }

    #[test]
    fn sort_order_decides_overlap_and_hidden_is_not_drawn() {
        let (mut e, red) = emulator_with_red_panel();
        let front = e.create_overlay("f", 0.1).unwrap();
        // 赤いパネルの左半分と同じ視線上の、より奥。
        e.set_overlay_pose(front, Pose::new(Vec3::new(-0.5, 0.0, -2.0), Quat::IDENTITY));
        e.submit_overlay_pixels(front, 1, 1, &[0, 255, 0, 255]);
        let at = e.project(Vec3::new(-0.25, 0.0, -1.0)).unwrap();
        // 奥にあっても後に作ったほうが手前に描かれる（同じ順位）。
        assert_eq!(pixel(&e.screenshot(), at), [0, 255, 0, 255]);
        e.set_overlay_sort(red, 1);
        assert_eq!(pixel(&e.screenshot(), at), [255, 0, 0, 255]);
        e.set_overlay_visible(red, false);
        assert_eq!(pixel(&e.screenshot(), at), [0, 255, 0, 255]);
    }

    #[test]
    fn overlay_alpha_fades_the_panel() {
        let (mut e, red) = emulator_with_red_panel();
        let at = e.project(Vec3::new(-0.25, 0.0, -1.0)).unwrap();
        assert!(e.set_overlay_alpha(red, 0.5));
        let half = pixel(&e.screenshot(), at);
        assert!(
            half[0] > half[1] && half[1] == half[2],
            "灰色に赤が半分混ざる"
        );
        e.set_overlay_alpha(red, 0.0);
        let gone = pixel(&e.screenshot(), at);
        assert!(gone[0] == gone[1] && gone[1] == gone[2]);
    }

    #[test]
    fn quad_seen_edge_on_from_the_eye_is_invisible() {
        // 目から伸びるレーザー: 面が目を通る。
        let (mut e, _) = emulator_with_red_panel();
        let blank = e.screenshot();
        let laser = e.create_overlay("l", 1.0).unwrap();
        let pose = Pose::new(
            Vec3::new(0.0, 0.0, -0.5),
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        );
        e.set_overlay_pose(laser, pose);
        e.submit_overlay_pixels(laser, 1, 1, &[0, 255, 0, 255]);
        assert_eq!(e.screenshot(), blank);
    }

    #[test]
    fn mouse_drives_right_hand_and_keys_drive_sticks() {
        let mut e = Emulator::headless(200, 100);
        let frame = e.poll();
        assert!(frame.emulated);
        assert_eq!(frame.controllers.len(), 1, "マウスが外なら左手だけ");
        assert_eq!(frame.controllers[0].hand, Hand::Left);

        let target = Vec3::new(0.2, 0.1, -1.0);
        e.point(e.project(target));
        e.set_button(Button::Trigger, true);
        e.set_stick(Hand::Left, Vec2::new(0.0, 1.0));
        let frame = e.poll();
        let right = &frame.controllers[1];
        assert_eq!(right.hand, Hand::Right);
        assert!(right.aim.forward().abs_diff_eq(target.normalize(), 1e-4));
        assert_eq!(right.input("trigger/value"), Some(1.0));
        assert_eq!(right.input("squeeze/value"), Some(0.0));
        let left = &frame.controllers[0];
        assert_eq!(left.input("thumbstick/y"), Some(1.0));
        // 左手は下を向いてパネルを指さない。
        assert!(left.aim.forward().abs_diff_eq(Vec3::NEG_Y, 1e-4));
    }
}
