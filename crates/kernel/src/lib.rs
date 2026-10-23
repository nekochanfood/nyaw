//! カーネル: 一点集中型の中枢。ブートローダー（`main`）が起こし、UI や画面はこれだけを相手にする。
//!
//! 最下部の [`subsystems`] が OS・ランタイム・入力・描画の差を吸収し、その上で設定とパネル管理を行う。
//! フレームの進行（[`Kernel::tick`]）・パネルへの操作（[`Kernel::apply`]）・描画結果の受け取り
//! （[`Kernel::submit`]）を提供する。UI や画面は下位層を直接組み立てない。

mod audio_panels;
pub mod config;
mod desktop_panels;
mod os_keyboard;
pub use audio_panels::AudioStatus;
pub use desktop_panels::DesktopStats;
pub use subsystems::{self, Subsystems};
pub use subsystems::{audio, desktop, rendering, vr};

use std::time::{Duration, Instant};
use subsystems::os;

pub use config::{Config, Language, LodQuality, PointerMode, ThemeMode};
pub use overlay::{self, Anchor, Overlay, Panel, PanelId, Parent, Pointer, PointerEvent, UiEvent};
pub use subsystems::ControllerState;
pub use vr::{Device, Hand, Pose, Runtime, Vec2, VrBackend, VrError};

/// [`Kernel::tick`] 1 回の内訳。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TickTiming {
    /// ランタイムの poll（vsync 待ち・合成を含む）。
    pub poll: Duration,
    /// オーバーレイ層（追従・ヒット判定・ポインタの配置・アニメーション）。
    pub overlay: Duration,
    /// poll のうち、ランタイムがこのスレッドで合成・表示にかけた時間（novr のエミュレーターだけ）。
    pub compositor: Duration,
}

/// 上位層（ビュー）からパネルへの操作要求。
#[derive(Debug, Clone, PartialEq)]
pub enum PanelCommand {
    Attach(Device, Pose),
    Detach,
    Lock(bool),
    /// 掴んで大きさを変えられるか（`Overlay::set_resizable`）。
    Resizable(bool),
    /// 再生中のメディアを操作する（再生・一時停止・曲送り・シーク・アプリを前に出す。[`Kernel::now_playing`]）。
    Media(subsystems::media::Command),
    /// 掴んだとき、手で動かして頭へ向ける基準（このパネルから見た姿勢。`Overlay::set_grab_anchor`）。
    GrabAnchor(Option<Pose>),
    /// このパネルのイン・アウトアニメーションを有効化・無効化する。
    Animated(bool),
    /// このパネルが出ている間、空間全体を暗くする（`Overlay::set_veil`）。
    Veil(bool),
    Close,
    /// 隠す・出し直す（`Overlay::set_hidden`。閉じずにビュー・配信・音を残す）。
    Hide(bool),
    /// その手のレーザーを描くパネルにする（ヒット判定の対象外になり、置き場所はオーバーレイが決める）。
    Pointer(Hand, Pointer),
    /// 解像度（UI の論理ピクセル）を変える。1m あたりのピクセル数を保って VR 上の大きさも変わる。
    Resize(u32, u32),
    /// 現在の縦横比のまま、VR 上の高さだけを揃える。
    FitHeight(f32),
    /// 今の頭から見た相対姿勢（`ui::PanelSpec::in_view` と同じく頭の傾きは無視）へ置き直す。取り付けは外す。
    Place(Pose),
    /// 親子関係を設定する（`None` で独立）。子は親に追従し、子が開いている間の親は暗くなって入力を受けない。
    Parent(Option<Parent>),
    /// このパネルを、その持ち主のナビゲーションバーにする（持ち主の下端に付き、頭から見た大きさを保つ）。
    NavBar(PanelId),
    /// デスクトップの配信を開始・停止する。第2引数は取得の上限 FPS（1〜240）。
    Stream(Option<desktop::Source>, u32),
    /// 配信しているウィンドウを OS で前に出してフォーカスする（OS のキーボード・マウスの操作が届くように）。
    /// ウィンドウの配信を始めたときはカーネルが自分で前に出すので、隠していた配信を出し直したときなどに使う。
    FocusSource,
    /// 配信中のウィンドウを OS で最小化する。
    MinimizeSource,
    /// このパネルのスピーカーで音を鳴らす（`true` で繰り返す）。読み込みは別スレッドで、いくつでも重ねて鳴らせる。
    Play(audio::Sound, bool),
    /// このパネルの中の点（パネルのピクセル、左上原点）から一度だけ音を鳴らす。キーボードのキーのように、押した所から聞こえる。
    /// `pitch` は速さ（1 = そのまま。上げると高く短くなる）。
    PlayAt {
        sound: audio::Sound,
        at: Vec2,
        pitch: f32,
    },
    /// このパネルを閉じるときにパネルの中央から一度鳴らす音。閉じた後も鳴り終わるまでその場所から聞こえる。
    CloseSound(audio::Sound),
    /// このパネルで鳴っている音をすべて止める。
    StopAudio,
    /// このパネルの音を置き場所から聞こえるようにする（既定）。`false` でそのまま左右に鳴らす。
    Spatial(bool),
    /// このパネルのスピーカーをモノラル・ステレオのどちらで鳴らすか（既定は音に合わせる）。
    SpeakerLayout(audio::Layout),
    /// 仮想キーボードの一操作を OS の前面のアプリへ送る（このパネルが OS を操作するパネルのとき）。
    TypeToOs {
        stroke: subsystems::keyboard::Stroke,
        layout: subsystems::keyboard::Layout,
    },
}

/// アプリ全体の設定の変更（[`Kernel::set`]）。パネルを問わず効き、`pointer_mode` 以外は設定ファイルへ書き戻される
/// （起動時の設定の場合のみ。テスト用のカーネルは書かない）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Setting {
    /// レーザーのエミュレート方式。
    PointerMode(PointerMode),
    Theme(ThemeMode),
    /// 画面の言語（`language`）。
    Language(Language),
    /// レーザーの手ブレ抑えの強さ（0 = 抑えない 〜 `overlay::POINTER_SMOOTHING_MAX`。`pointer_smoothing`）。
    PointerSmoothing(f32),
    /// 手ブレ抑えを緩める角度（度。`pointer_release_deg`）。
    PointerRelease(f32),
    /// レーザーの見える長さ（m。`laser_length`）。
    LaserLength(f32),
    /// コントローラを持たない手の調整値（`[hand]`）。範囲の外の値は範囲に収める。
    Hand(subsystems::input::HandTuning),
}

/// アプリ全体の設定の今の値（[`Kernel::settings`]）。ビューの表示用。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Settings {
    pub pointer_mode: PointerMode,
    pub theme: ThemeMode,
    pub language: Language,
    pub pointer_smoothing: f32,
    pub pointer_release_deg: f32,
    pub laser_length: f32,
    pub hand: subsystems::input::HandTuning,
}

/// パネルの状態。ビューの表示用。全体の設定は [`Settings`]。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PanelStatus {
    pub runtime: Option<Runtime>,
    pub attached: Option<Device>,
    pub locked: bool,
    pub animated: bool,
    /// 見えているトラッカーの ID（取り付け先の候補）。
    pub trackers: Vec<String>,
    /// メニューボタンを押している。
    pub menu: bool,
    /// ショートカットボタン（左手の X）を二回続けて押した回数（全パネル共通。起動から数える）。
    /// 押した瞬間を取りこぼさないようカーネルが毎 tick 数え、何をするかは画面が決める（wrist はダッシュボードを開く・閉じる）。
    pub double_press: u32,
    /// どのパネルにも当たっていない所でトリガーを押した回数（全パネル共通。起動から数える）。何をするかは画面が決める
    /// （ダッシュボードは出ていれば隠す）。
    pub outside_press: u32,
    /// パネルを掴んでいる。
    pub grabbed: bool,
    /// 閉じられた。
    pub closed: bool,
    /// 隠している（[`PanelCommand::Hide`]）。
    pub hidden: bool,
    /// 描画解像度の段階（[`Kernel::panel_lod`]）。
    pub lod: u8,
    /// 親パネル（このパネルが子のとき）。
    pub parent: Option<PanelId>,
    /// レーザーが指している（`Overlay::pointed`。ナビゲーションバーは持ち主を指していても）。
    pub pointed: bool,
    pub desktop: Option<desktop::Status>,
    /// 配信中なら、直近一秒の統計（最初の一秒は `None`）。
    pub desktop_stats: Option<DesktopStats>,
    /// このパネルのスピーカー。
    pub audio: AudioStatus,
}

/// 中枢。本番は `Kernel`（= `Kernel<Box<dyn VrBackend>>`）、テストは具象のバックエンド（モック等）を持てる。
pub struct Kernel<B: VrBackend = Box<dyn VrBackend>> {
    pub config: Config,
    /// テストがモックを直接いじるために公開している。本番コードはメソッド経由で使う。
    pub subsystems: Subsystems<B>,
    overlay: Overlay,
    timing: TickTiming,
    pointer_mode: PointerMode,
    /// テーマの変更を書き戻す設定ファイル。`boot` だけが設定する（テスト用のカーネルは実ファイルを触らない）。
    config_path: Option<std::path::PathBuf>,
    /// 進めたフレームの数（[`Self::panel_frame_due`]）。
    frame: u64,
    /// パネルごとの LOD（[`Self::panel_lod`]）。tick ごとに前の値から決める。
    lods: Vec<u8>,
    desktop_panels: std::collections::HashMap<PanelId, desktop_panels::DesktopPanel>,
    /// 音の出力。テスト用のカーネルはデバイスを開かず（`audio::Engine::offline`）、テストが `render` で回す。
    pub audio: audio::Engine,
    /// 音を鳴らした・設定したパネルのスピーカー。
    speakers: std::collections::HashMap<PanelId, audio_panels::SpeakerConfig>,
    /// パネルごとの、次に使う点の音のスピーカー（[`PanelCommand::PlayAt`]）。
    point_slots: std::collections::HashMap<PanelId, u64>,
    /// 閉じるときに鳴らす音（[`PanelCommand::CloseSound`]）。
    close_sounds: std::collections::HashMap<PanelId, audio::Sound>,
    /// ショートカットボタンの二回連続押し（[`PanelStatus::double_press`]）。
    shortcut: DoublePress,
    /// 骨格だけの手のショートカット（親指と小指）の長押し。二回押しと同じく [`PanelStatus::double_press`] に数える。
    hand_shortcut: LongPress,
    /// OS の前面のアプリのキーボードの監視と入力（[`Kernel::os_keyboard`]。使い始めるまで起こさない）。
    os_keyboard: Option<os::keyboard::Watcher>,
    /// 最後の tick の電池（[`Kernel::batteries`]）。
    batteries: Vec<vr::Battery>,
    /// 再生中のメディアの見張り（[`Kernel::now_playing`]）。`boot` だけが起こす（テスト用のカーネルは OS に触らない）。
    media: Option<subsystems::media::Media>,
}

/// 二回続けて押したと見なす、押下と次の押下の間隔の上限。
pub const DOUBLE_PRESS: Duration = Duration::from_millis(250);
/// 骨格だけの手のショートカット（親指と小指を付ける）を、この時間押し続けると一回と数える。指を付け直す二回押しは、
/// ハンドトラッキングでは離したかどうかが曖昧で数え損ねるので、手は長押しにする。
pub const HAND_LONG_PRESS: Duration = Duration::from_millis(500);

/// 押し続けて [`HAND_LONG_PRESS`] たったら一回数える（離すまで次は数えない）。
#[derive(Debug, Default)]
struct LongPress {
    since: Option<Instant>,
    fired: bool,
    count: u32,
}

impl LongPress {
    fn update(&mut self, held: bool, now: Instant) {
        if !held {
            (self.since, self.fired) = (None, false);
            return;
        }
        let since = *self.since.get_or_insert(now);
        if !self.fired && now.duration_since(since) >= HAND_LONG_PRESS {
            self.fired = true;
            self.count += 1;
            logger::debug!("hand shortcut held", count = self.count);
        }
    }
}

/// 押下の立ち上がりを見て、[`DOUBLE_PRESS`] 以内に二回続いたら数える。三回目は次の一回目として数え直す。
#[derive(Debug, Default)]
struct DoublePress {
    held: bool,
    first: Option<Instant>,
    count: u32,
}

impl DoublePress {
    fn update(&mut self, held: bool, now: Instant) {
        let pressed = held && !self.held;
        self.held = held;
        if !pressed {
            return;
        }
        let since = self.first.take().map(|t| now.duration_since(t));
        match since {
            Some(d) if d <= DOUBLE_PRESS => self.count += 1,
            _ => self.first = Some(now),
        }
        logger::debug!("shortcut button pressed", since_first = ?since, count = self.count);
    }
}

/// LOD の切り替え距離を決めた基準の密度（1m あたりの論理ピクセル。既定のパネル 800px / 0.8m）。
/// 拡大して密度が下がったパネルは、そのぶん近くにあるものとして LOD を決める（見た目の粗さは距離 / 密度で決まる）。
const LOD_PX_PER_M: f32 = 1000.0;
/// LOD を下げる（近づいて解像度を上げる）のは、閾値のこの割合まで近づいてから。
const LOD_HYSTERESIS: f32 = 0.9;

impl Kernel {
    /// ランタイムを選んで起動する。`requested` は `"--novr"` / `"novr"` のような指定（コマンドライン > 設定ファイル。決めるのは呼び出し側）。
    /// 指定なしで VR ランタイムが無ければ novr で起動する（`novr` フィーチャーがあれば常に使える）。指定したのに繋がらなければエラー。
    pub fn boot(requested: Option<&str>, config: Config) -> Result<Self, VrError> {
        let prefer = match requested.map(runtime_name).as_deref() {
            None | Some("") => None,
            Some("novr") => Some(Runtime::NoVr),
            Some("openxr") => Some(Runtime::OpenXr),
            Some("openvr") => Some(Runtime::OpenVr),
            // 読めない指定を「指定なし」として扱うと、VR ランタイムに繋ぎに行って SteamVR を起こしてしまう。
            Some(_) => {
                logger::error!("unknown runtime", requested = requested.unwrap_or_default());
                return Err(VrError::NoRuntime);
            }
        };
        let vr = match vr::connect(prefer, config.gpu.emulator) {
            Ok(b) => b,
            Err(e) if prefer.is_none() => {
                logger::warn!("failed to connect to VR runtime, starting novr", error = e);
                // `novr` フィーチャーなしのビルドにはエミュレーターが無く、ここで失敗しうる。
                vr::connect(Some(Runtime::NoVr), config.gpu.emulator)?
            }
            Err(e) => {
                logger::error!("failed to connect to VR runtime", runtime = ?prefer, error = e);
                return Err(e);
            }
        };
        logger::info!("runtime selected", runtime = ?vr.runtime());
        let mut kernel = Self::with_backend(vr, config);
        kernel.config_path = config::path();
        kernel.audio = audio::Engine::open();
        kernel.media = Some(subsystems::media::Media::start());
        Ok(kernel)
    }
}

/// ランタイムの指定を比べられる形にする（`--NoVr` / `no-vr` / `no_vr` → `novr`）。
fn runtime_name(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

impl<B: VrBackend> Kernel<B> {
    /// 接続済みのバックエンドで起動する（テスト用。本番は [`Kernel::boot`]）。
    pub fn with_backend(vr: B, config: Config) -> Self {
        // フレームの周期を作る短い待ちが丸められないように。
        os::precise_timers();
        // ゲームが GPU を使い切っていても、パネルの描画と読み戻しが後回しにされないように。
        if !os::high_gpu_priority() {
            logger::warn!("cannot raise GPU scheduling priority");
        }
        let mut subsystems = Subsystems::with_backend(vr);
        subsystems.hand = config.hand.clamped();
        let mut overlay = Overlay::new();
        if let Some(s) = config.pointer_smoothing {
            overlay.set_pointer_smoothing(s);
        }
        if let Some(deg) = config.pointer_release_deg {
            overlay.set_pointer_release(deg);
        }
        if let Some(l) = config.laser_length {
            overlay.set_laser_length(l);
        }
        let pointer_mode = config.pointer_mode.unwrap_or_default();
        // タッチではスライドがパネルからはみ出しても続くよう、押している間はポインタを掴んでおく。
        overlay.set_capture(pointer_mode == PointerMode::Touch);
        Self {
            pointer_mode,
            config_path: None,
            config,
            subsystems,
            overlay,
            timing: TickTiming::default(),
            frame: 0,
            lods: Vec::new(),
            desktop_panels: Default::default(),
            audio: audio::Engine::offline(48_000, false),
            speakers: Default::default(),
            point_slots: Default::default(),
            close_sounds: Default::default(),
            shortcut: DoublePress::default(),
            hand_shortcut: LongPress::default(),
            os_keyboard: None,
            batteries: Vec::new(),
            media: None,
        }
    }

    pub fn runtime(&self) -> Runtime {
        self.subsystems.backend.runtime()
    }

    /// レーザーのエミュレート方式。UI がポインタイベントをマウス・タッチのどちらとして流すかを決める。
    pub fn pointer_mode(&self) -> PointerMode {
        self.pointer_mode
    }

    pub fn settings(&self) -> Settings {
        Settings {
            pointer_mode: self.pointer_mode,
            theme: self.config.theme,
            language: self.config.language,
            pointer_smoothing: self.overlay.pointer_smoothing(),
            pointer_release_deg: self.overlay.pointer_release(),
            laser_length: self.overlay.laser_length(),
            hand: self.subsystems.hand,
        }
    }

    /// アプリ全体の設定を変える。
    pub fn set(&mut self, setting: Setting) {
        let (key, value) = match setting {
            Setting::PointerMode(m) => {
                self.pointer_mode = m;
                self.overlay.set_capture(m == PointerMode::Touch);
                return;
            }
            Setting::Theme(t) => {
                self.config.theme = t;
                ("theme", config::value(t))
            }
            Setting::Language(l) => {
                self.config.language = l;
                ("language", config::value(l))
            }
            Setting::PointerSmoothing(s) => {
                self.overlay.set_pointer_smoothing(s);
                let s = self.overlay.pointer_smoothing();
                self.config.pointer_smoothing = Some(s);
                ("pointer_smoothing", config::value(s))
            }
            Setting::PointerRelease(deg) => {
                self.overlay.set_pointer_release(deg);
                let deg = self.overlay.pointer_release();
                self.config.pointer_release_deg = Some(deg);
                ("pointer_release_deg", config::value(deg))
            }
            Setting::LaserLength(l) => {
                self.overlay.set_laser_length(l);
                let l = self.overlay.laser_length();
                self.config.laser_length = Some(l);
                ("laser_length", config::value(l))
            }
            Setting::Hand(h) => {
                let h = h.clamped();
                self.subsystems.hand = h;
                self.config.hand = h;
                ("hand", config::value(h))
            }
        };
        if let Some(path) = &self.config_path
            && let Err(e) = config::save_at(path, key, value)
        {
            logger::warn!(
                "failed to save to config file",
                key = key,
                path = path.display(),
                error = e
            );
        }
    }

    pub fn add_panel(
        &mut self,
        key: &str,
        pose: Pose,
        size: Vec2,
        resolution: (u32, u32),
    ) -> Result<PanelId, VrError> {
        self.overlay
            .add_panel(&mut self.subsystems.backend, key, pose, size, resolution)
    }

    pub fn panel(&self, id: PanelId) -> &Panel {
        self.overlay.panel(id)
    }

    /// `owner` に付いている開いたナビゲーションバー。
    pub fn nav_bar(&self, owner: PanelId) -> Option<PanelId> {
        self.overlay.present_ids().iter().copied().find(|&i| {
            let p = self.overlay.panel(i);
            !p.closed && p.nav_bar_of == Some(owner)
        })
    }

    /// 距離による描画解像度の段階（LOD0〜3）。Mid は 2 / 4 / 8 m で切り替える（下げるのは [`LOD_HYSTERESIS`] だけ近づいてから）。
    /// 距離は密度で換算する（[`LOD_PX_PER_M`]）。2 倍に拡大したパネルは半分の距離として扱う。
    /// [`Self::tick`] で決めた値（まだ tick していないパネルはその場で求める）。
    pub fn panel_lod(&self, id: PanelId) -> u8 {
        self.lods
            .get(id)
            .copied()
            .unwrap_or_else(|| self.lod_for(id, None))
    }

    /// 前の段階 `prev` から、距離で次の段階を決める。境目の前後で行き来しないよう、下げる側は閾値を縮める。
    fn lod_for(&self, id: PanelId, prev: Option<u8>) -> u8 {
        if self
            .panel(id)
            .pointer
            .is_some_and(|(_, part)| part == Pointer::Tooltip)
        {
            return 0;
        }
        let near = match self.config.lod_quality {
            LodQuality::Off => return 0,
            LodQuality::Low => 1.0,
            LodQuality::Mid => 2.0,
            LodQuality::High => 4.0,
        };
        let p = self.panel(id);
        let density = p.resolution.0 as f32 / p.size.x / LOD_PX_PER_M;
        let distance = p.pose.position.distance(self.hmd().position) * density;
        let count = |scale: f32| {
            [near, near * 2.0, near * 4.0]
                .into_iter()
                .filter(|&threshold| distance >= threshold * scale)
                .count() as u8
        };
        let (up, down) = (count(1.0), count(LOD_HYSTERESIS));
        match prev {
            // 上げるのは閾値ちょうどで、下げるのは縮めた閾値を割ってから。
            Some(prev) => prev.clamp(up, down),
            None => up,
        }
    }

    /// このフレームでパネルを描き直してよいか。LOD0 は毎フレーム、LOD n は 2^n フレームに 1 回。
    /// パネルごとにずらし、間引いたパネルの描き直しが同じフレームに重ならないようにする。
    pub fn panel_frame_due(&self, id: PanelId) -> bool {
        (self.frame + id as u64).is_multiple_of(1 << self.panel_lod(id))
    }

    /// 1 フレーム進める。ランタイムを poll し、パネルの追従・掴み移動とレーザーのヒット判定を行って UI イベントを返す。
    pub fn tick(&mut self) -> Vec<UiEvent> {
        let t = Instant::now();
        self.frame += 1;
        let frame = self.subsystems.poll();
        let polled = Instant::now();
        self.poll_desktop();
        // ボタン（左手の X）は二回押し、骨格だけの手（向きが分かる）は長押し。
        let held = |hand: bool| {
            frame
                .controllers
                .iter()
                .any(|c| c.shortcut && c.facing.is_some() == hand)
        };
        self.shortcut.update(held(false), polled);
        self.hand_shortcut.update(held(true), polled);
        self.batteries.clone_from(&frame.batteries);
        let mut events = self.overlay.tick_frame(&mut self.subsystems.backend, frame);
        self.route_desktop(&mut events);
        self.update_audio();
        // 段階が変わるとパネルの描き直し・描画先とランタイムのテクスチャの作り直しが走るので、境目で行き来させない。
        let old_len = self.lods.len();
        self.lods.resize(self.overlay.len(), 0);
        for &id in self.overlay.present_ids() {
            if !self.overlay.panel(id).closed {
                let prev = (id < old_len).then_some(self.lods[id]);
                self.lods[id] = self.lod_for(id, prev);
            }
        }
        self.timing = TickTiming {
            poll: polled - t,
            overlay: polled.elapsed(),
            compositor: self.subsystems.backend.compositor_time(),
        };
        events
    }

    /// 最後の [`tick`](Self::tick) の内訳（プロファイラ用）。
    pub fn timing(&self) -> TickTiming {
        self.timing
    }

    /// フレームの周期が決まっていれば（novr）その長さ。プロファイラがドロップを数える基準に使う。
    pub fn frame_interval(&self) -> Option<Duration> {
        self.subsystems.backend.frame_interval()
    }

    /// `hand` が押している間のポインタのキャプチャを外す（タッチの長押しでドラッグになったとき。ほかのパネルへ運べるように）。
    pub fn release_capture(&mut self, hand: Hand) {
        self.overlay.release_capture(hand);
    }

    /// パネルが見えているか（ほかのパネルに隠れていない・視野の中）。ランタイムが直前の合成から教えてくれればそれ（novr）、
    /// 無ければ頭の向きと視野で見積もる。見えないパネルの描き直しを省く（カリング）のに使う。
    pub fn panel_seen(&self, panel: PanelId) -> bool {
        let p = self.overlay.panel(panel);
        !p.closed
            && self
                .subsystems
                .backend
                .overlay_seen(p.overlay_id())
                .unwrap_or_else(|| self.overlay.in_view(panel))
    }

    /// ランタイムが同時に出せるオーバーレイの数の上限（[`VrBackend::max_overlays`]）。
    pub fn max_overlays(&self) -> Option<usize> {
        self.subsystems.backend.max_overlays()
    }

    /// 最後の tick の HMD の姿勢。
    pub fn hmd(&self) -> Pose {
        self.overlay.hmd()
    }

    /// 最後の tick で骨格が取れていた手（スケルタル入力）。
    pub fn hands(&self) -> &[vr::HandSkeleton] {
        self.overlay.hands()
    }

    /// 再生中（一時停止中も含む）のメディア。何も再生していなければ `None`。読み込みは別スレッドで、最後に読んだもの。
    pub fn now_playing(&self) -> Option<std::sync::Arc<subsystems::media::NowPlaying>> {
        self.media.as_ref().and_then(|m| m.now())
    }

    /// 最後の tick で電池の残量を申告していたデバイス。
    pub fn batteries(&self) -> &[vr::Battery] {
        &self.batteries
    }

    /// `panel` そのものをレーザーで指している手（[`Overlay::pointing_hands`]）。
    pub fn pointing_hands(&self, panel: PanelId) -> Vec<Hand> {
        self.overlay.pointing_hands(panel)
    }

    /// 最後の tick の手ごとの入力の状態（骨格だけの手は判定に使った値 `metrics` 付き）。
    pub fn controllers(&self) -> &[ControllerState] {
        self.overlay.controllers()
    }

    /// `hand` のツールチップをカーソルの上下どちらに出すか（`Overlay::tooltip_anchor`）。
    pub fn tooltip_anchor(&self, hand: Hand) -> Anchor {
        self.overlay.tooltip_anchor(hand)
    }

    /// `hand` でドラッグしている間、その見た目を幅 `width_m` で出す（`Overlay::set_drag`）。`None` で隠す。
    pub fn set_drag(&mut self, hand: Hand, width_m: Option<f32>) {
        self.overlay.set_drag(hand, width_m)
    }

    /// ランタイムが終了を求めている（SteamVR の終了など）。呼び出し側はループを抜ける。
    pub fn quit_requested(&self) -> bool {
        self.overlay.quit_requested()
    }

    /// パネルの描画結果（RGBA8）をランタイムへ送る。
    pub fn submit(&mut self, panel: PanelId, width: u32, height: u32, rgba: &[u8]) {
        if self.desktop_streaming(panel) {
            return;
        }
        self.overlay
            .submit(&mut self.subsystems.backend, panel, width, height, rgba)
    }

    pub fn apply(&mut self, panel: PanelId, cmd: PanelCommand) {
        match cmd {
            PanelCommand::Attach(device, offset) => self.overlay.attach(panel, device, offset),
            PanelCommand::Detach => self.overlay.detach(panel),
            PanelCommand::Lock(locked) => self.overlay.set_locked(panel, locked),
            PanelCommand::Animated(animated) => {
                self.overlay
                    .set_animated(&mut self.subsystems.backend, panel, animated)
            }
            PanelCommand::Veil(veil) => self.overlay.set_veil(panel, veil),
            PanelCommand::Resizable(resizable) => self.overlay.set_resizable(panel, resizable),
            PanelCommand::Media(command) => {
                if let Some(media) = &self.media {
                    media.send(command);
                }
            }
            PanelCommand::GrabAnchor(anchor) => self.overlay.set_grab_anchor(panel, anchor),
            PanelCommand::Hide(hidden) => {
                self.overlay
                    .set_hidden(&mut self.subsystems.backend, panel, hidden)
            }
            PanelCommand::Close => {
                if let Some(sound) = self.close_sounds.remove(&panel) {
                    let (w, h) = self.overlay.panel(panel).resolution;
                    self.play_audio_at(panel, sound, Vec2::new(w as f32, h as f32) / 2.0, 1.0);
                }
                self.overlay.close(&mut self.subsystems.backend, panel)
            }
            PanelCommand::Pointer(hand, part) => {
                self.overlay
                    .set_pointer(&mut self.subsystems.backend, panel, hand, part)
            }
            PanelCommand::Resize(w, h) => {
                self.overlay
                    .set_resolution(&mut self.subsystems.backend, panel, (w, h))
            }
            PanelCommand::FitHeight(height) => {
                self.overlay
                    .fit_height(&mut self.subsystems.backend, panel, height)
            }
            PanelCommand::Parent(parent) => {
                self.overlay
                    .set_parent(&mut self.subsystems.backend, panel, parent)
            }
            PanelCommand::NavBar(owner) => {
                self.overlay
                    .set_nav_bar(&mut self.subsystems.backend, panel, owner)
            }
            PanelCommand::Place(offset) => {
                self.overlay.detach(panel);
                self.overlay.set_pose(panel, self.hmd().leveled() * offset);
            }
            PanelCommand::Stream(source, frame_rate) => {
                self.start_desktop(panel, source, frame_rate)
            }
            PanelCommand::FocusSource => self.focus_desktop(panel),
            PanelCommand::MinimizeSource => self.minimize_desktop(panel),
            PanelCommand::Play(sound, looping) => self.play_audio(panel, sound, looping),
            PanelCommand::PlayAt { sound, at, pitch } => {
                self.play_audio_at(panel, sound, at, pitch)
            }
            PanelCommand::CloseSound(sound) => {
                self.close_sounds.insert(panel, sound);
            }
            PanelCommand::StopAudio => self.stop_audio(panel),
            PanelCommand::Spatial(spatial) => {
                self.configure_speaker(panel, |c| c.spatial = spatial)
            }
            PanelCommand::SpeakerLayout(layout) => {
                self.configure_speaker(panel, |c| c.layout = layout)
            }
            PanelCommand::TypeToOs { stroke, layout } => self.type_to_os(panel, stroke, layout),
        }
    }

    pub fn status(&self, panel: PanelId) -> PanelStatus {
        let p = self.overlay.panel(panel);
        PanelStatus {
            runtime: Some(self.subsystems.backend.runtime()),
            attached: p.attached.as_ref().map(|(d, _)| d.clone()),
            locked: p.locked,
            animated: p.animated,
            trackers: self.overlay.trackers().to_vec(),
            menu: self.overlay.menu_held(),
            double_press: self.shortcut.count + self.hand_shortcut.count,
            outside_press: self.overlay.outside_presses(),
            grabbed: self.overlay.grabbed(panel),
            closed: p.closed,
            hidden: p.hidden,
            lod: self.panel_lod(panel),
            parent: p.parent.map(|q| q.panel),
            pointed: self.overlay.pointed(panel),
            desktop: self.desktop_panels.get(&panel).map(|p| p.status.clone()),
            desktop_stats: self.desktop_panels.get(&panel).and_then(|p| p.stats),
            audio: self.audio_status(panel),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vr::mock::MockBackend;

    fn kernel() -> (Kernel<MockBackend>, PanelId) {
        let mut k = Kernel::with_backend(MockBackend::default(), Config::default());
        let p = k.add_panel("t", Pose::IDENTITY, Vec2::ONE, (4, 4)).unwrap();
        (k, p)
    }

    #[test]
    fn double_press_counts_pairs_within_the_window() {
        let t = Instant::now();
        let ms = |n| t + Duration::from_millis(n);
        let mut d = DoublePress::default();
        // 押しっぱなしは一回。
        for (held, at) in [(true, 0), (true, 50), (false, 100), (true, 200)] {
            d.update(held, ms(at));
        }
        assert_eq!(d.count, 1);
        // 三回目は数え直しの一回目。間が空きすぎた二回目も一回目になる。
        d.update(false, ms(250));
        d.update(true, ms(300));
        d.update(false, ms(350));
        let late = ms(300) + Duration::from_millis(251);
        d.update(true, late);
        assert_eq!(d.count, 1);
        d.update(false, late + Duration::from_millis(50));
        d.update(true, late + Duration::from_millis(250));
        assert_eq!(d.count, 2);
    }

    /// 手の長押しは 0.5 秒で一回。押し続けても増えず、途中で離すと数え直し。
    #[test]
    fn long_press_counts_once_per_hold() {
        let t = Instant::now();
        let ms = |n| t + Duration::from_millis(n);
        let mut l = LongPress::default();
        for (held, at) in [
            (true, 0),
            (true, 300),
            (false, 400),
            (true, 500),
            (true, 900),
        ] {
            l.update(held, ms(at));
        }
        assert_eq!(l.count, 0, "離したので 0.5 秒続いていない");
        l.update(true, ms(1000));
        assert_eq!(l.count, 1);
        l.update(true, ms(3000));
        assert_eq!(l.count, 1, "押し続けても一回");
        l.update(false, ms(3100));
        l.update(true, ms(3200));
        l.update(true, ms(3700));
        assert_eq!(l.count, 2);
    }

    /// 手の調整は範囲に収めて入力層へ渡り、設定として読める。
    #[test]
    fn hand_setting_reaches_the_input_layer() {
        let (mut k, _) = kernel();
        let h = subsystems::input::HandTuning {
            pinch_mm: 1000.0,
            grip_deg: 75.0,
            ..Default::default()
        };
        k.set(Setting::Hand(h));
        let expected = subsystems::input::HandTuning {
            pinch_mm: subsystems::input::HandTuning::PINCH_MM.1,
            ..h
        };
        assert_eq!(k.settings().hand, expected);
        assert_eq!(k.subsystems.hand, expected);
        assert_eq!(k.config.hand, expected);
    }

    /// 左手の X ボタンを二回押すと、どのパネルの状態でも数が増える。
    #[test]
    fn x_pressed_twice_shows_in_status() {
        let (mut k, p) = kernel();
        let x = |k: &mut Kernel<MockBackend>, held: bool| {
            k.subsystems.backend.frame.controllers = vec![vr::RawController {
                hand: Hand::Left,
                profile: "oculus/touch_controller".into(),
                aim: Pose::IDENTITY,
                inputs: vec![("x/click".into(), if held { 1.0 } else { 0.0 })],
            }];
            k.tick();
        };
        for held in [true, false, true, false] {
            x(&mut k, held);
        }
        assert_eq!(k.status(p).double_press, 1);
    }

    #[cfg(feature = "novr")]
    #[test]
    fn boot_novr_by_name() {
        for name in ["novr", "--novr", "--no-vr", "NoVr", "no_vr"] {
            for gpu in [false, true] {
                let mut config = Config::default();
                config.gpu.emulator = gpu;
                let k = Kernel::boot(Some(name), config).unwrap();
                assert_eq!(k.runtime(), Runtime::NoVr);
            }
        }
    }

    #[test]
    fn boot_rejects_unknown_runtime_instead_of_connecting_to_vr() {
        assert!(Kernel::boot(Some("--nvr"), Config::default()).is_err());
    }

    #[cfg(feature = "novr")]
    #[test]
    fn boot_without_request_always_succeeds() {
        // 実ランタイムの有無に依らず、指定なしなら novr にフォールバックして起動できる
        assert!(Kernel::boot(None, Config::default()).is_ok());
    }

    #[test]
    fn apply_lock_shows_in_status() {
        let (mut k, p) = kernel();
        assert!(!k.status(p).locked);
        k.apply(p, PanelCommand::Lock(true));
        assert!(k.status(p).locked);
    }

    #[test]
    fn place_detaches_and_moves_relative_to_head() {
        let (mut k, p) = kernel();
        k.apply(p, PanelCommand::Attach(Device::Hmd, Pose::IDENTITY));
        let to = Pose::new(vr::Vec3::new(1.0, 0.0, -2.0), vr::Quat::IDENTITY);
        k.apply(p, PanelCommand::Place(to));
        assert_eq!(k.status(p).attached, None);
        assert_eq!(k.panel(p).pose, k.hmd().leveled() * to);
    }

    #[test]
    fn place_comes_in_front_of_the_face_upright_and_lying() {
        let (mut k, p) = kernel();
        let to = Pose::new(vr::Vec3::new(0.0, 0.0, -0.8), vr::Quat::IDENTITY);
        let head = vr::Vec3::new(0.2, 1.6, 0.1);
        // 立っていて首を少し傾けている: 正面の目の高さに、まっすぐ立てて出る。
        // 横向き・仰向けに寝転がっている: 顔の正面に、頭の上を上にして出る。
        for (q, upright) in [
            (
                vr::Quat::from_rotation_y(0.8) * vr::Quat::from_rotation_z(0.3),
                true,
            ),
            (
                vr::Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
                false,
            ),
            (
                vr::Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                false,
            ),
        ] {
            k.subsystems.backend.frame.hmd = Pose::new(head, q);
            k.tick();
            k.apply(p, PanelCommand::Place(to));
            let pose = k.panel(p).pose;
            let ahead = head + q * vr::Vec3::new(0.0, 0.0, -0.8);
            let up = pose.orientation * vr::Vec3::Y;
            if upright {
                assert!((pose.position.y - head.y).abs() < 1e-5, "{q}");
                assert!(up.abs_diff_eq(vr::Vec3::Y, 1e-5), "{q}");
            } else {
                assert!(
                    pose.position.abs_diff_eq(ahead, 1e-5),
                    "{q} {}",
                    pose.position
                );
                assert!(up.abs_diff_eq(q * vr::Vec3::Y, 1e-5), "{q}");
            }
            // どちらでも表面は頭を向く。
            let to_head = (head - pose.position).normalize();
            assert!((pose.orientation * vr::Vec3::Z).dot(to_head) > 0.99, "{q}");
        }
    }

    /// 境目の前後で行き来しない: 上げるのは閾値で、下げるのは閾値の 9 割を割ってから。
    #[test]
    fn lod_does_not_flap_at_the_threshold() {
        let (mut k, p) = lod_kernel();
        k.config.lod_quality = LodQuality::Low;
        let pose = k.panel(p).pose;
        for (distance, lod) in [(1.0, 1), (0.95, 1), (1.0, 1), (0.85, 0), (0.95, 0)] {
            k.subsystems.backend.frame.hmd.position = pose.position + vr::Vec3::Z * distance;
            k.tick();
            assert_eq!(k.panel_lod(p), lod, "{distance}");
        }
    }

    /// 基準の密度（[`LOD_PX_PER_M`]）のパネルを 1 枚。
    fn lod_kernel() -> (Kernel<MockBackend>, PanelId) {
        let mut k = Kernel::with_backend(MockBackend::default(), Config::default());
        let p = k
            .add_panel("t", Pose::IDENTITY, Vec2::ONE, (1000, 1000))
            .unwrap();
        (k, p)
    }

    /// Issue #51: 遠くで LOD が効いたまま拡大すると粗いまま残っていた。拡大して密度が下がれば、同じ距離でも LOD は下がる。
    #[test]
    fn scaled_up_panel_gets_a_finer_lod_at_the_same_distance() {
        let (mut k, p) = lod_kernel();
        k.config.lod_quality = LodQuality::Low;
        // 同じ解像度のまま 4 倍に拡大したパネル（密度 1/4）を同じ所に。
        let big = k
            .add_panel("big", Pose::IDENTITY, Vec2::splat(4.0), (1000, 1000))
            .unwrap();
        k.subsystems.backend.frame.hmd.position = vr::Vec3::Z * 3.0;
        k.tick();
        assert_eq!(k.panel_lod(p), 2);
        assert_eq!(k.panel_lod(big), 0, "3m / 4 = 0.75m 相当");
    }

    #[test]
    fn tooltip_keeps_full_resolution_when_far_from_the_head() {
        let (mut k, p) = lod_kernel();
        k.apply(p, PanelCommand::Pointer(vr::Hand::Right, Pointer::Tooltip));
        k.subsystems.backend.frame.hmd.position = vr::Vec3::Z * 100.0;
        k.tick();
        assert_eq!(k.panel_lod(p), 0);
    }

    #[test]
    fn distance_lod_tracks_camera_and_preserves_panel_geometry() {
        let (mut k, p) = lod_kernel();
        let pose = k.panel(p).pose;
        let resolution = k.panel(p).resolution;
        let size = k.panel(p).size;
        k.config.lod_quality = LodQuality::Off;
        k.subsystems.backend.frame.hmd.position = pose.position + vr::Vec3::Z * 100.0;
        k.tick();
        assert_eq!(k.panel_lod(p), 0);
        for (quality, near) in [
            (LodQuality::Low, 1.0),
            (LodQuality::Mid, 2.0),
            (LodQuality::High, 4.0),
        ] {
            k.config.lod_quality = quality;
            for (distance, lod) in [
                (0.0, 0),
                (near - 0.01, 0),
                (near, 1),
                (near * 2.0, 2),
                (near * 4.0, 3),
                (100.0, 3),
                (0.5, 0),
            ] {
                k.subsystems.backend.frame.hmd.position = pose.position + vr::Vec3::Z * distance;
                k.tick();
                assert_eq!(k.panel_lod(p), lod, "{quality:?}: {distance}");
                let due = (0..8)
                    .filter(|_| {
                        k.tick();
                        k.panel_frame_due(p)
                    })
                    .count();
                assert_eq!(due, 8 >> lod, "{quality:?}: {distance}");
                assert_eq!(k.panel(p).resolution, resolution);
                assert_eq!(k.panel(p).size, size);
            }
        }
    }

    #[test]
    fn apply_resize_changes_resolution_and_vr_width() {
        let (mut k, p) = kernel();
        k.apply(p, PanelCommand::Resize(8, 2));
        assert_eq!(k.panel(p).resolution, (8, 2));
        assert_eq!(k.panel(p).size, Vec2::new(2.0, 0.5));
        assert_eq!(k.subsystems.backend.overlays[0].width_m, 2.0);
    }

    #[test]
    fn pointer_mode_starts_from_config_and_switches() {
        let config = Config {
            pointer_mode: Some(PointerMode::Touch),
            ..Default::default()
        };
        let mut k = Kernel::with_backend(MockBackend::default(), config);
        assert_eq!(k.settings().pointer_mode, PointerMode::Touch);
        k.set(Setting::PointerMode(PointerMode::Mouse));
        assert_eq!(k.pointer_mode(), PointerMode::Mouse);
        assert_eq!(
            kernel().0.pointer_mode(),
            PointerMode::Touch,
            "defaults to touch"
        );
    }

    #[test]
    fn theme_starts_from_config_and_switches_without_touching_files() {
        let config = Config {
            theme: ThemeMode::Light,
            ..Default::default()
        };
        let mut k = Kernel::with_backend(MockBackend::default(), config);
        assert_eq!(k.settings().theme, ThemeMode::Light);
        k.set(Setting::Theme(ThemeMode::Dark));
        assert_eq!(k.settings().theme, ThemeMode::Dark);
    }

    #[test]
    fn language_starts_from_config_and_persists_to_config_path() {
        let config = Config {
            language: Language::Ja,
            ..Default::default()
        };
        let mut k = Kernel::with_backend(MockBackend::default(), config);
        assert_eq!(k.settings().language, Language::Ja);
        let path =
            std::env::temp_dir().join(format!("nyaw-kernel-lang-{}.toml", std::process::id()));
        k.config_path = Some(path.clone());
        k.set(Setting::Language(Language::En));
        assert_eq!(k.settings().language, Language::En);
        let saved = config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(saved.language, Language::En);
    }

    #[test]
    fn theme_setting_persists_to_config_path() {
        let path = std::env::temp_dir().join(format!("nyaw-kernel-{}.toml", std::process::id()));
        let (mut k, _) = kernel();
        k.config_path = Some(path.clone());
        k.set(Setting::Theme(ThemeMode::Light));
        let saved = config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(saved.theme, ThemeMode::Light);
    }

    #[test]
    fn submit_reaches_backend() {
        let (mut k, p) = kernel();
        k.apply(p, PanelCommand::Animated(false));
        k.submit(p, 1, 1, &[1, 2, 3, 4]);
        let tex = k.subsystems.backend.overlays[0].texture.as_ref().unwrap();
        assert_eq!(tex.pixel(0, 0), [1, 2, 3, 4]);
    }

    #[test]
    fn animation_command_opts_out_only_the_selected_panel() {
        let (mut k, p) = kernel();
        let other = k
            .add_panel("other", Pose::IDENTITY, Vec2::ONE, (4, 4))
            .unwrap();
        k.apply(p, PanelCommand::Animated(false));
        assert!(!k.status(p).animated && k.status(other).animated);
        k.submit(p, 1, 1, &[255; 4]);
        k.submit(other, 1, 1, &[255; 4]);
        assert_eq!(k.subsystems.backend.overlays[p].width_m, 1.0);
        assert_eq!(k.subsystems.backend.overlays[other].width_m, 0.75);
        k.apply(p, PanelCommand::Close);
        assert!(k.status(p).closed && !k.subsystems.backend.overlays[p].visible);
        k.apply(other, PanelCommand::Animated(false));
        assert_eq!(
            k.subsystems.backend.overlays[other]
                .texture
                .as_ref()
                .unwrap()
                .pixel(0, 0),
            [255; 4]
        );
    }
}
