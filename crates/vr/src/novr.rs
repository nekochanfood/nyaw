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

use std::time::{Duration, Instant};

use minifb::{Key, MouseButton, MouseMode, Window, WindowOptions};

use crate::compositor::{Draw, Gpu};

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
/// フレームの周期（約 90Hz）。VR ランタイムの vsync 待ちと同じく、poll はここまで待つ。
const FRAME: Duration = Duration::from_nanos(1_000_000_000 / 90);
/// ホイール 1 刻みで右手のスティックを倒す量（1 フレームだけ）。オーバーレイはスティックの値をそのまま行数として送るので、
/// 1 刻み = 3 行（一般的なホイールの既定）になる。
// ponytail: 刻みの大きさ（Windows は 120 単位、ほかは OS ごと）は見ず向きだけ。滑らかなスクロールのホイールでは速すぎるかもしれない。
const WHEEL: f32 = 3.0;
/// GPU で描くとき、見えたかを求める CPU の合成の縮小率（縦横これで割る）。
const SEEN_SCALE: u32 = 8;
/// 合成に使うスレッドの上限（画面を横帯に分けて並べて塗る）。
const MAX_THREADS: usize = 8;
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
    /// 直前の合成で 1 画素でも見えた（[`VrBackend::overlay_seen`]）。
    seen: bool,
    sort: u32,
    /// 幅, 高さ, RGBA。
    texture: Option<(u32, u32, Vec<u8>)>,
}

/// novr のエミュレーター。本番は [`crate::connect`] がウィンドウ付きで作り、testkit は [`Emulator::headless`] で作る。
pub struct Emulator {
    /// GPU コンポジタ。ウィンドウに直接描く（使えなければ CPU で合成して minifb に渡す）。
    /// ウィンドウを使うので、ウィンドウより先に落とす（フィールドはこの順で落ちる）。
    gpu: Option<Gpu>,
    /// `None` かつ `!headless` なら最初の poll で開く。
    window: Option<Window>,
    headless: bool,
    /// ウィンドウを出すが、入力はウィンドウから読まない（[`Emulator::scripted_window`]）。
    scripted: bool,
    size: (u32, u32),
    /// 合成した絵（0RGB）。
    pixels: Vec<u32>,
    /// 背景の市松模様（`size` の大きさ。変わったら作り直す）。
    background: Vec<u32>,
    /// GPU で描くときに、見えたかを求めるための縮小した合成の置き場。
    seen_pixels: Vec<u32>,
    /// クアッドごとの、GPU へ上げ直すテクスチャがあるか。
    uploads: Vec<bool>,
    dirty: bool,
    quads: Vec<Quad>,
    /// マウスの位置（ウィンドウのピクセル、左上原点）。ウィンドウの外なら `None`（右手が見えない）。
    pointer: Option<Vec2>,
    trigger: bool,
    grab: bool,
    /// 左手・右手のスティック。
    sticks: [Vec2; 2],
    quit: bool,
    /// 次のフレームの始まり（ウィンドウ付きの poll はここまで眠る）。
    next_frame: Option<Instant>,
    /// 直前の合成と表示にかかった時間（[`VrBackend::compositor_time`]）。
    composite_time: Duration,
}

impl Emulator {
    /// ウィンドウ付き。ウィンドウは最初の poll で開き、閉じると終了を求める。入力はウィンドウから受ける。
    pub(crate) fn windowed() -> Self {
        Self::new(SIZE, false)
    }

    /// ウィンドウを出して本番と同じく合成・表示する（GPU が使えれば GPU）が、入力はウィンドウからではなく
    /// [`Emulator::point`] などで注入する（ベンチマーク用。本番と同じ合成のコストを測る）。
    pub fn scripted_window(width: u32, height: u32) -> Self {
        Self {
            scripted: true,
            ..Self::new((width, height), false)
        }
    }

    /// ウィンドウを出さない（testkit 用）。入力は [`Emulator::point`] などで注入し、絵は [`Emulator::screenshot`] で見る。
    pub fn headless(width: u32, height: u32) -> Self {
        Self::new((width, height), true)
    }

    fn new(size: (u32, u32), headless: bool) -> Self {
        Self {
            gpu: None,
            window: None,
            headless,
            scripted: false,
            size,
            pixels: Vec::new(),
            background: Vec::new(),
            seen_pixels: Vec::new(),
            uploads: Vec::new(),
            dirty: true,
            quads: Vec::new(),
            pointer: None,
            trigger: false,
            grab: false,
            sticks: [Vec2::ZERO; 2],
            quit: false,
            next_frame: None,
            composite_time: Duration::ZERO,
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
        project_at(self.size, p)
    }

    /// 画面上の点を通る、目からの視線（z = -1 に正規化）。[`Emulator::project`] の逆。
    fn ray(&self, at: Vec2) -> Vec3 {
        let (k, b) = lens_at(self.size);
        (at * k + b).extend(-1.0)
    }

    /// 今の画面（幅, 高さ, RGBA）。ユーザーに見えている絵。
    pub fn screenshot(&mut self) -> (u32, u32, Vec<u8>) {
        if self.gpu.is_some() {
            // GPU で描いているときは CPU の絵が無いので、CPU で合成し直す（次のフレームは GPU でも描き直す）。
            self.dirty = true;
            self.render();
            self.dirty = true;
        } else {
            self.render();
        }
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
            match Window::new(TITLE, self.size.0 as usize, self.size.1 as usize, opts) {
                Ok(mut w) => {
                    // 周期は poll が自分で作る（minifb の待ちを合成の時間に混ぜない）。
                    w.set_target_fps(0);
                    if std::env::var_os("NYAW_EMULATOR_CPU").is_none() {
                        match Gpu::new(&w, self.size) {
                            Ok(gpu) => {
                                logger::info!("エミュレーターは GPU で合成する");
                                self.gpu = Some(gpu);
                            }
                            Err(e) => {
                                logger::warn!("GPU で合成できないので CPU で合成する", error = e)
                            }
                        }
                    }
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
            if let Some(gpu) = &mut self.gpu {
                gpu.resize(self.size);
            }
        }
        if self.scripted {
            return;
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

    /// 変化があれば合成し直す（かかった時間は [`VrBackend::compositor_time`] で返す）。ウィンドウ付きでは poll が呼ぶ。
    /// ウィンドウの無いとき（testkit・ベンチマーク）に、本番と同じ合成のコストを掛けたいなら毎フレーム呼ぶ。
    pub fn composite(&mut self) {
        let t = Instant::now();
        self.render();
        self.composite_time = t.elapsed();
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
        let mut pixels = std::mem::take(&mut self.pixels);
        let seen = self.rasterize(self.size, &mut pixels, Some(&self.background));
        self.pixels = pixels;
        self.mark_seen(&seen);
    }

    /// 変化があれば GPU で描いて表示する。見えたかは縮小した CPU の合成（縦横 1/[`SEEN_SCALE`]）で求める。
    fn render_gpu(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let order = self.order();
        let mut draws = Vec::with_capacity(order.len());
        for i in order {
            let Some(size) = self.quad_size(i) else {
                continue;
            };
            let q = &self.quads[i];
            let upload = std::mem::take(&mut self.uploads[i]);
            draws.push(Draw {
                quad: i,
                pose: q.pose,
                size,
                alpha: q.alpha,
                upload: upload
                    .then(|| {
                        q.texture
                            .as_ref()
                            .map(|(w, h, rgba)| (*w, *h, rgba.as_slice()))
                    })
                    .flatten(),
            });
        }
        if let Some(gpu) = &mut self.gpu
            && let Err(e) = gpu.draw(&draws, FOV_Y, NEAR)
        {
            logger::debug!("エミュレーターの画面を出せない", error = e);
            self.dirty = true;
        }
        drop(draws);
        let small = (
            (self.size.0 / SEEN_SCALE).max(1),
            (self.size.1 / SEEN_SCALE).max(1),
        );
        let mut scratch = std::mem::take(&mut self.seen_pixels);
        let seen = self.rasterize(small, &mut scratch, None);
        self.seen_pixels = scratch;
        self.mark_seen(&seen);
    }

    /// 直前の合成で見えたかを付け直す。
    fn mark_seen(&mut self, seen: &[(usize, bool)]) {
        for q in &mut self.quads {
            q.seen = false;
        }
        for &(i, v) in seen {
            self.quads[i].seen = v;
        }
    }

    /// 描くクアッドを、奥から手前へ重ねる順に並べる（OpenVR と同じく描画順、同じなら頭（原点）から遠い順。
    /// 奥行きを押し引きすると前後が入れ替わる）。
    fn order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.quads.len())
            .filter(|&i| {
                let q = &self.quads[i];
                q.visible && q.alpha > 0.0 && q.texture.is_some()
            })
            .collect();
        let far = |i: usize| -self.quads[i].pose.position.length();
        order.sort_by(|&a, &b| {
            (self.quads[a].sort, far(a))
                .partial_cmp(&(self.quads[b].sort, far(b)))
                .unwrap()
        });
        order
    }

    /// 大きさ `size` の画面に CPU で合成する（`background` が無ければ塗らず、見えたかだけ出す）。
    /// 手前から奥へ塗り（画素ごとにここまでの不透明度を持ち、不透明になった画素では奥のクアッドを計算しない。重なりのカリング）、
    /// 画面を横帯に分けて帯ごとに別のスレッドで塗る。クアッドごとに、ほかに隠されていない画素があったかを返す。
    fn rasterize(
        &self,
        size: (u32, u32),
        pixels: &mut Vec<u32>,
        background: Option<&[u32]>,
    ) -> Vec<(usize, bool)> {
        let (w, h) = size;
        let mut order = self.order();
        order.reverse();
        pixels.resize((w * h) as usize, 0);
        let rasters: Vec<Raster> = order
            .into_iter()
            .filter_map(|i| self.raster(i, size))
            .collect();
        let threads = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .clamp(1, MAX_THREADS);
        let rows = h.div_ceil(threads as u32).max(1);
        let band = (rows * w) as usize;
        let fill = |(k, px): (usize, &mut [u32])| -> Vec<bool> {
            let y0 = k as u32 * rows;
            let y1 = y0 + (px.len() / w as usize) as u32;
            px.fill(0);
            let seen = rasters.iter().map(|r| r.fill(px, y0, y1, w)).collect();
            // 残った透けている分に背景を敷く。
            if let Some(background) = background {
                let bg = &background[y0 as usize * w as usize..][..px.len()];
                for (p, b) in px.iter_mut().zip(bg) {
                    *p = under_background(*p, *b);
                }
            }
            seen
        };
        let seen: Vec<Vec<bool>> = if threads == 1 {
            pixels.chunks_mut(band).enumerate().map(fill).collect()
        } else {
            std::thread::scope(|s| {
                let handles: Vec<_> = pixels
                    .chunks_mut(band)
                    .enumerate()
                    .map(|chunk| s.spawn(move || fill(chunk)))
                    .collect();
                handles
                    .into_iter()
                    .map(|h| h.join().expect("合成のスレッドが落ちた"))
                    .collect()
            })
        };
        rasters
            .iter()
            .enumerate()
            .map(|(j, r)| (r.quad, seen.iter().any(|band| band[j])))
            .collect()
    }

    /// クアッド `i` を塗る準備（画面上の範囲と、視線との交点を足し込みで出す係数）。写らなければ `None`。
    fn raster(&self, i: usize, size: (u32, u32)) -> Option<Raster<'_>> {
        let q = &self.quads[i];
        let (tw, th, tex) = q.texture.as_ref()?;
        let (tw, th) = (*tw, *th);
        let screen = size;
        let size = self.quad_size(i)?;
        let (ux, uy, n) = (
            q.pose.orientation * Vec3::X,
            q.pose.orientation * Vec3::Y,
            q.pose.orientation * Vec3::Z,
        );
        let c = q.pose.position;
        let nc = n.dot(c);
        if nc.abs() < NEAR || tw == 0 || th == 0 {
            return None;
        }
        // 4 隅が写る範囲だけを走査する。全部の隅が目の後ろなら写らない。一部が後ろなら全面。
        let (w, h) = screen;
        let world = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(sx, sy)| c + ux * size.x / 2.0 * sx + uy * size.y / 2.0 * sy);
        if world.iter().all(|p| p.z > -NEAR) {
            return None;
        }
        let corners = world.map(|p| project_at(screen, p));
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
        let (k, b) = lens_at(screen);
        let dot = |a: Vec3| [a.x * k.x, a.x * b.x - a.z, a.y * k.y, a.y * b.y];
        Some(Raster {
            quad: i,
            tex,
            tw,
            th,
            size,
            nc,
            n: dot(n),
            u: dot(ux),
            v: dot(uy),
            cu: c.dot(ux),
            cv: c.dot(uy),
            alpha: (q.alpha * 255.0).round() as u32,
            rect: (x0, y0, x1.min(w), y1.min(h)),
        })
    }

    /// クアッド `i` の大きさ（m）。高さを固定していなければテクスチャの縦横比から。テクスチャが無ければ `None`。
    fn quad_size(&self, i: usize) -> Option<Vec2> {
        let q = &self.quads[i];
        let (tw, th, _) = q.texture.as_ref()?;
        Some(Vec2::new(
            q.width_m,
            q.height_m
                .unwrap_or(q.width_m * *th as f32 / (*tw).max(1) as f32),
        ))
    }

    fn quad(&mut self, id: OverlayId) -> &mut Quad {
        self.dirty = true;
        &mut self.quads[id.0 as usize]
    }
}

/// 大きさ `size` の画面で、頭の座標系の点が映る画面上の点。目の後ろなら `None`。
fn project_at(size: (u32, u32), p: Vec3) -> Option<Vec2> {
    if p.z > -NEAR {
        return None;
    }
    let (k, b) = lens_at(size);
    Some((Vec2::new(p.x, p.y) / -p.z - b) / k)
}

/// 大きさ `size` の画面で、点 `at` を通る視線の x, y（z = -1）が `at * k + b` になる係数。
fn lens_at(size: (u32, u32)) -> (Vec2, Vec2) {
    let (w, h) = (size.0 as f32, size.1 as f32);
    let t = (FOV_Y / 2.0).tan();
    let k = Vec2::new(2.0 * t / h, -2.0 * t / h);
    (k, Vec2::new(-t * w / h, t))
}

/// 乗算済みの ARGB（手前に塗った分）の下に、背景（0RGB）を残った透けている分だけ敷いた 0RGB。
fn under_background(p: u32, bg: u32) -> u32 {
    let a = p >> 24;
    if a == 255 {
        return p & 0x00ff_ffff;
    }
    let [_, r, g, b] = p.to_be_bytes().map(u32::from);
    let [_, br, bgg, bb] = bg.to_be_bytes().map(u32::from);
    let mix = |c: u32, d: u32| (c + d * (255 - a) / 255).min(255);
    (mix(r, br) << 16) | (mix(g, bgg) << 8) | mix(b, bb)
}

/// 1 枚のクアッドを塗るための、フレームごとに決まる値（[`Emulator::raster`]）。
struct Raster<'a> {
    /// どのクアッドか（`Emulator::quads` の添字）。
    quad: usize,
    tex: &'a [u8],
    tw: u32,
    th: u32,
    size: Vec2,
    nc: f32,
    /// 法線・横・縦の向きと視線の内積を、画面の (x, y) の 1 次式にした係数 [x の係数, 定数, y の係数, y の定数]。
    n: [f32; 4],
    u: [f32; 4],
    v: [f32; 4],
    cu: f32,
    cv: f32,
    alpha: u32,
    /// 画面上の範囲 (x0, y0, x1, y1)。
    rect: (u32, u32, u32, u32),
}

impl Raster<'_> {
    /// 画面の行 `band_y0..band_y1` を受け持つ帯 `px`（幅 `w`）の、ここまで手前に塗った分の下に敷く（`px` は乗算済みの ARGB）。
    /// 手前に隠されていない画素が 1 つでもあれば `true`（自分の透明度は問わない）。
    fn fill(&self, px: &mut [u32], band_y0: u32, band_y1: u32, w: u32) -> bool {
        let mut seen = false;
        let (x0, y0, x1, y1) = self.rect;
        let (tw_f, th_f) = (self.tw as f32, self.th as f32);
        let px0 = x0 as f32 + 0.5;
        for y in y0.max(band_y0)..y1.min(band_y1) {
            let py = y as f32 + 0.5;
            let start = |c: [f32; 4]| c[1] + c[2] * py + c[3] + c[0] * px0;
            let (mut dn, mut du, mut dv) = (start(self.n), start(self.u), start(self.v));
            let line = ((y - band_y0) * w) as usize;
            for x in x0..x1 {
                let (pn, pu, pv) = (dn, du, dv);
                dn += self.n[0];
                du += self.u[0];
                dv += self.v[0];
                let dst = &mut px[line + x as usize];
                let covered = *dst >> 24;
                // 手前で不透明になった画素は計算しない。
                if covered == 255 || pn.abs() < 1e-9 {
                    continue;
                }
                let t = self.nc / pn;
                if t < NEAR {
                    continue;
                }
                let u = (pu * t - self.cu) / self.size.x + 0.5;
                let v = 0.5 - (pv * t - self.cv) / self.size.y;
                if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                    continue;
                }
                let tx = ((u * tw_f) as u32).min(self.tw - 1);
                let ty = ((v * th_f) as u32).min(self.th - 1);
                let s = ((ty * self.tw + tx) * 4) as usize;
                // 手前に隠されていない画素がある（自分が透明でも、描き直せば見えうるので見えたとする）。
                seen = true;
                let a = u32::from(self.tex[s + 3]) * self.alpha / 255;
                // この画素へのこのクアッドの寄与（手前で隠れた分を除く）。
                let k = a * (255 - covered) / 255;
                if k == 0 {
                    continue;
                }
                let [r, g, b] = [self.tex[s], self.tex[s + 1], self.tex[s + 2]].map(u32::from);
                let [_, dr, dg, db] = dst.to_be_bytes().map(u32::from);
                let add = |d: u32, s: u32| (d + s * k / 255).min(255);
                *dst = ((covered + k).min(255) << 24)
                    | (add(dr, r) << 16)
                    | (add(dg, g) << 8)
                    | add(db, b);
            }
        }
        seen
    }
}

impl VrBackend for Emulator {
    fn runtime(&self) -> Runtime {
        Runtime::NoVr
    }

    /// ウィンドウ付きでは [`FRAME`] の周期まで待ってから、前のフレームの絵を出して入力を読む。
    fn poll(&mut self) -> Frame {
        if !self.headless {
            let now = Instant::now();
            let next = self.next_frame.unwrap_or(now);
            if next > now {
                std::thread::sleep(next - now);
            }
            // 間に合わなかったフレームは取り戻そうとせず、今から数え直す。
            self.next_frame = Some(next.max(Instant::now() - FRAME) + FRAME);
            self.read_window();
            let t = Instant::now();
            if self.gpu.is_some() {
                self.render_gpu();
                // GPU が直接ウィンドウに出すので、minifb にはメッセージを回させるだけ。
                if let Some(win) = &mut self.window {
                    win.update();
                }
            } else {
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
            self.composite_time = t.elapsed();
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
            seen: false,
            sort: 0,
            texture: None,
        });
        self.uploads.push(false);
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
        self.uploads[id.0 as usize] = true;
    }

    fn compositor_time(&self) -> Duration {
        self.composite_time
    }

    fn frame_interval(&self) -> Option<Duration> {
        Some(FRAME)
    }

    fn overlay_seen(&self, id: OverlayId) -> Option<bool> {
        Some(self.quads[id.0 as usize].seen)
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
        // 同じ順位なら近いほうが手前に描かれる（後から作っても奥の緑は隠れる）。
        assert_eq!(pixel(&e.screenshot(), at), [255, 0, 0, 255]);
        // 赤を奥へ押すと前後が入れ替わる。
        e.set_overlay_pose(red, Pose::new(Vec3::new(0.0, 0.0, -3.0), Quat::IDENTITY));
        assert_eq!(pixel(&e.screenshot(), at), [0, 255, 0, 255]);
        e.set_overlay_pose(red, Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY));
        e.set_overlay_sort(front, 0);
        e.set_overlay_sort(red, 1);
        assert_eq!(pixel(&e.screenshot(), at), [255, 0, 0, 255]);
        e.set_overlay_visible(red, false);
        assert_eq!(pixel(&e.screenshot(), at), [0, 255, 0, 255]);
    }

    #[test]
    fn panel_hidden_behind_an_opaque_one_is_not_seen() {
        let (mut e, red) = emulator_with_red_panel();
        // 赤（左半分は不透明・右半分は半透明）の手前に、赤より大きい不透明な板。
        let front = e.create_overlay("f", 2.0).unwrap();
        e.set_overlay_pose(front, Pose::new(Vec3::new(0.0, 0.0, -0.5), Quat::IDENTITY));
        e.submit_overlay_pixels(front, 1, 1, &[0, 255, 0, 255]);
        e.screenshot();
        assert_eq!(e.overlay_seen(front), Some(true));
        assert_eq!(e.overlay_seen(red), Some(false), "隠れたパネルは見えない");
        // 板を退けると見える。
        e.set_overlay_visible(front, false);
        e.screenshot();
        assert_eq!(e.overlay_seen(red), Some(true));
        // 目の後ろに回ると見えない。
        e.set_overlay_pose(red, Pose::new(Vec3::new(0.0, 0.0, 1.0), Quat::IDENTITY));
        e.screenshot();
        assert_eq!(e.overlay_seen(red), Some(false));
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
