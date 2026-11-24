//! パネルの起動仕様（[`PanelSpec`]）。画面と VR 空間での配置・サイズ・親子関係・ナビゲーションバーの有無を定義する。

use super::*;

/// 親パネルから見たスポーン位置（親の中心から手前 10cm）。
const SPAWN_GAP: Vec3 = Vec3::new(0.0, 0.0, 0.1);
/// タイピング位置パネル（[`PanelSpec::typing`]）の照準を目からどれだけ下げるか（m。コントローラ非接続時）。
const TYPING_AIM_DROP: f32 = 0.15;
/// タイピング位置パネルの中心を手（コントローラ）の高さからどれだけ上げるか（m）。手を動かさず、
/// 手首を少し起こすだけでポイントできる高さ。
const TYPING_ABOVE_HANDS: f32 = 0.1;
/// タイピング位置パネルの中心の、目から見た高さ下限（m）。手を完全に下ろして開いても視認できる位置に収める。
const TYPING_LOWEST: f32 = -0.6;
/// 起立時、タイピング位置パネルの上端を目の高さからどれだけ下げるか（m）。手を上げていても目線より下に配置する。
const TYPING_CLEARANCE: f32 = 0.05;
/// 仰臥時、タイピング位置パネルの上端を視線からどれだけ下げるか（m。頭基準）。視界内に収めつつ
/// 正面のパネルに重ねない。仰臥時は手の高さが不安定なため、手には合わせない。
const TYPING_LYING_CLEARANCE: f32 = 0.25;
/// タイピング位置パネルの上端と、呼び出し元（とそのナビゲーションバー・ドック先）の下端の、頭基準の角度間隔（rad）。
/// 視界内で呼び出し元の直下に、重なりなく表示される。
const TYPING_PANEL_GAP: f32 = 3.0 * std::f32::consts::PI / 180.0;
/// タイピング位置パネルの面を向ける先の、目から手への補間率（0 = 目、1 = 手）。読みやすさ（頭方向）を重視しつつ、
/// 手からのレーザーが浅角で当たらないよう手方向にも少し向ける。
const TYPING_TOWARD_HANDS: f32 = 0.3;

type PanelBuilder = dyn FnOnce(PanelId, &mut Window, &mut App) -> AnyView;

/// ナビゲーションバーの最小化ボタンで呼ぶ処理（[`PanelSpec::minimizable`]）。引数は最小化対象パネル。
pub(crate) type Minimize = std::rc::Rc<dyn Fn(PanelId, &mut App)>;

/// 画面と VR 空間での配置仕様。異なる画面型を同一の起動リストに格納できる。
pub struct PanelSpec {
    pose: Pose,
    from: Option<PanelId>,
    /// 起動時にタイピング位置に配置する（[`Self::typing`]）。値はパネル中心から見た、呼び出し元の中心に左右を合わせる点の水平位置（m）。
    pub(crate) typing: Option<f32>,
    /// `from` から見た配置位置（[`Self::offset`]）。未設定なら `from` の手前に少し離して重ねる。
    offset: Option<Pose>,
    /// 配置位置とサイズを `from` のナビゲーションバーに合わせる（[`Self::over_nav_bar`]）。
    over_nav_bar: bool,
    /// `pose` を起動時の頭部（向きは水平方向のみ）からの相対姿勢とする。
    in_view: bool,
    pub(crate) size: Vec2,
    pub(crate) resolution: (u32, u32),
    /// 起動時のアタッチ先デバイスと相対姿勢。
    attach: Option<(Device, Pose)>,
    /// 起動時からグラブ不可。
    pub(crate) locked: bool,
    /// 頭部にゆるく追従する（[`Self::lazy_follow`]）。
    lazy_follow: Option<kernel::LazyFollow>,
    /// 起動時点からロックダウンを取得する（[`Self::lockdown`]）。
    lockdown: bool,
    /// グラブでリサイズ可能（既定。[`Self::without_resize`] で無効化する）。
    resizable: bool,
    /// 角を掴んで解像度を変えられる範囲（[`Self::corner_resize`]）と、縦横比を保つか（[`Self::keep_aspect`]）。
    corner_resize: Option<kernel::ResizeLimits>,
    keep_aspect: bool,
    /// アタッチ中にグラブで移動可能な範囲（[`Self::reach`]）。
    reach: Option<f32>,
    /// 全体設定に依存しないポインタモード（[`Self::pointer_mode`]）。
    pointer_mode: Option<PointerMode>,
    /// グラブ時に手で移動する基準点（[`Self::grab_anchor`]）。
    grab_anchor: Option<Pose>,
    /// 親子関係の設定（[`Self::child`]）。
    parent: Option<kernel::Parent>,
    animated: bool,
    /// 表示中に空間全体を暗転する（[`Self::with_veil`]）。
    veil: bool,
    /// レーザー描画用パネル。
    pointer: Option<(Hand, Pointer)>,
    /// ナビゲーションバーに表示するタイトル。
    title: Option<Title>,
    /// ナビゲーションバーの有無（既定は有り。[`Self::without_nav_bar`] で無効化する）。
    nav_bar: bool,
    /// このパネル自身がナビゲーションバーの場合、その持ち主パネル。
    nav_bar_of: Option<PanelId>,
    /// ナビゲーションバーの左端に OS キーボードトグルを表示する（[`Self::with_os_keyboard`]）。
    os_keyboard: bool,
    /// ナビゲーションバーの左端にドック着脱ボタンを表示する（[`Self::dockable`]）。
    dockable: bool,
    /// ナビゲーションバーの右端を閉じるボタンの代わりに最小化ボタンにする（[`Self::minimizable`]）。
    minimize: Option<Minimize>,
    /// 閉じるボタンの隣に最小化ボタンを表示する（[`Self::source_minimizable`]）。
    source_minimize: Option<Minimize>,
    build: Box<PanelBuilder>,
}

/// ナビゲーションバーのウィンドウサイズ（px）。帯（[`design::components::NAV_BAR_HEIGHT`]）の周囲に影の余白を含む。
pub(crate) const NAV_BAR_PX: (u32, u32) = (
    480 + 2 * design::components::PANEL_MARGIN as u32,
    design::components::NAV_BAR_HEIGHT as u32 + 2 * design::components::PANEL_MARGIN as u32,
);

/// ナビゲーションバーの頭部から 1m 先での幅（m）。帯の高さがツールチップの 2 倍に見える縮尺。
/// オーバーレイ層が距離に比例して拡大するため、距離によらず見た目のサイズは一定。
pub(crate) const NAV_BAR_M: f32 = NAV_BAR_PX.0 as f32 * 2.0 * design::components::TOOLTIP_HEIGHT_M
    / design::components::NAV_BAR_HEIGHT;

/// ナビゲーションバーウィンドウ（影余白込み）の頭部から 1m 先での高さ（m）。オーバーレイ層が距離に比例して拡大する。
pub const NAV_BAR_HEIGHT_M: f32 = NAV_BAR_M * NAV_BAR_PX.1 as f32 / NAV_BAR_PX.0 as f32;

impl PanelSpec {
    /// 親を持たないパネル（起動時に開く常駐パネル、テストの初期パネル）。`pose` に配置する。
    /// 他のパネルから開くものは [`Self::spawn`] / [`Self::child`] / [`Self::popover`] で作成する（既定で親の手前に
    /// 重なる）。別の位置に配置する場合のみ [`Self::in_view`] 等で明示的に指定する。
    pub fn root<V: Render>(
        pose: Pose,
        build: impl FnOnce(PanelId, &mut Window, &mut Context<V>) -> V + 'static,
    ) -> Self {
        Self {
            pose,
            from: None,
            typing: None,
            offset: None,
            over_nav_bar: false,
            in_view: false,
            size: Vec2::new(0.8, 0.6),
            resolution: PANEL_PX,
            attach: None,
            locked: false,
            lazy_follow: None,
            lockdown: false,
            resizable: true,
            corner_resize: None,
            keep_aspect: false,
            reach: None,
            pointer_mode: None,
            grab_anchor: None,
            parent: None,
            animated: true,
            veil: false,
            pointer: None,
            title: None,
            nav_bar: true,
            nav_bar_of: None,
            os_keyboard: false,
            dockable: false,
            minimize: None,
            source_minimize: None,
            build: Box::new(move |panel, window, cx| cx.new(|cx| build(panel, window, cx)).into()),
        }
    }

    /// パネル `from` から開くパネル。既定で `from` の手前に少し離して重ねる（[`Self::offset`] で変更可能）。
    pub fn spawn<V: Render>(
        from: PanelId,
        build: impl FnOnce(PanelId, &mut Window, &mut Context<V>) -> V + 'static,
    ) -> Self {
        Self {
            from: Some(from),
            ..Self::root(Pose::IDENTITY, build)
        }
    }

    /// `from` に固定したポップオーバー（メニュー等）として表示する。`from` を暗転せず入力もブロックせず、外側クリックで閉じる。
    /// 配置位置は [`Self::offset`] で指定する。ナビゲーションバーは付けない。
    pub fn popover<V: Render>(
        from: PanelId,
        build: impl FnOnce(PanelId, &mut Window, &mut Context<V>) -> V + 'static,
    ) -> Self {
        Self {
            parent: Some(kernel::Parent {
                panel: from,
                carry: true,
                dismiss: true,
                docked: true,
                ..Default::default()
            }),
            nav_bar: false,
            ..Self::spawn(from, build)
        }
    }

    /// 起動時の `from`（[`Self::spawn`] / [`Self::child`] / [`Self::popover`] の親）から見た配置位置。
    pub fn offset(mut self, offset: Pose) -> Self {
        self.offset = Some(offset);
        self
    }

    /// [`Self::offset`] を `from` のナビゲーションバー基準の配置位置にし、バーの現在サイズ（頭部からの距離で変動）に合わせて
    /// 配置位置とサイズを拡縮する（[`Self::sized`] はバーと同様に 1m 先でのサイズで記述する）。バー上に表示するポップオーバー用。
    pub(crate) fn over_nav_bar(mut self) -> Self {
        self.over_nav_bar = true;
        self
    }

    /// [`Self::over_nav_bar`] の場合、`from` のナビゲーションバーと、その現在サイズの 1m 先でのサイズに対する比率。
    fn nav_bar_scale<B: VrBackend>(&self, kernel: &Kernel<B>) -> Option<(PanelId, f32)> {
        let bar = kernel.nav_bar(self.from.filter(|_| self.over_nav_bar)?)?;
        Some((
            bar,
            kernel.panel(bar).size.x / (NAV_BAR_M * kernel.ui_scale()),
        ))
    }

    /// `from` の子として表示する（ダイアログ等）。親に追従し、表示中の親は暗転して入力を受け付けない。
    /// 子をグラブした場合、`carry` なら親も連動し、そうでなければ子のみが分離する。
    pub fn child<V: Render>(
        from: PanelId,
        carry: bool,
        build: impl FnOnce(PanelId, &mut Window, &mut Context<V>) -> V + 'static,
    ) -> Self {
        Self {
            parent: Some(kernel::Parent {
                panel: from,
                carry,
                dismiss: true,
                hud: false,
                ..Default::default()
            }),
            ..Self::spawn(from, build)
        }
    }

    /// 子の外側をクリックしても閉じないようにする（既定は閉じる）。親がなければ何もしない。
    pub fn without_dismiss(mut self) -> Self {
        if let Some(p) = &mut self.parent {
            p.dismiss = false;
        }
        self
    }

    /// 親の左上に重ねる表示専用の子にする（[`kernel::Parent::hud`]）。親がなければ何もしない。
    pub fn hud(mut self) -> Self {
        if let Some(p) = &mut self.parent {
            (p.hud, p.dismiss) = (true, false);
        }
        self
    }

    /// 親子関係の作成要求（親がなければ `None`）。起動直後に [`PanelCommand::Parent`] で適用する。
    pub fn parent(&self) -> Option<kernel::Parent> {
        self.parent
    }

    /// 起動時の目の前に配置する。`offset` は頭部からの相対姿勢（頭部の傾きは無視し、パネルは直立で配置する）。
    /// トラッキング空間の原点はユーザーの位置と一致しないため、召喚するパネルにはこれを使う。
    /// 親の手前に重ねる既定（[`Self::spawn`]）から外すことになるため、呼び出し側にその理由を記載する。
    pub fn in_view(mut self, offset: Pose) -> Self {
        (self.pose, self.from, self.in_view) = (offset, None, true);
        self
    }

    /// サイズ（m）と解像度（px）を指定する。既定は 0.8m × 0.6m / 800 × 600。
    pub fn sized(mut self, size: Vec2, resolution: (u32, u32)) -> Self {
        (self.size, self.resolution) = (size, resolution);
        self
    }

    /// 解像度のアスペクト比を保ったまま、縦のサイズを `height` m にする（レイアウトを変えずにサイズのみ調整する）。
    pub fn fit_height(mut self, height: f32) -> Self {
        self.size = Vec2::new(
            height * self.resolution.0 as f32 / self.resolution.1 as f32,
            height,
        );
        self
    }

    /// 起動時の頭部前方の手を軽く伸ばした位置（タイピング位置）に配置する。高さと仰角のみ頭部（と手）から決定し、
    /// 左右の向きと位置は呼び出し元（[`Self::spawn`] の `from`）に合わせる。`center` はパネル中心からの水平位置（m）で、
    /// その点を呼び出し元の中心の左右に合わせる（キーボードならホームポジション）。親子関係や追従は作成しない。
    pub fn typing(mut self, center: f32) -> Self {
        self.typing = Some(center);
        self
    }

    /// 起動時にデバイスへアタッチする。
    pub fn attached(mut self, device: Device, offset: Pose) -> Self {
        self.attach = Some((device, offset));
        self
    }

    /// 起動時点からロックダウンを取得する（[`PanelCommand::Lockdown`]）。解放（または閉じる）まで、このパネルは閉じられず、
    /// ダッシュボード等のシェルも開閉を抑制する。解放はビューが `Lockdown(false)` をキューイングして行う。
    pub fn lockdown(mut self) -> Self {
        self.lockdown = true;
        self
    }

    /// 頭部の向き（左右のみ）と移動にゆるく追従する（[`kernel::LazyFollow`]）。他のパネルでも使用可能。
    pub fn lazy_follow(mut self, follow: kernel::LazyFollow) -> Self {
        self.lazy_follow = Some(follow);
        self
    }

    /// 起動時からグラブ不可にする。
    pub fn locked(mut self) -> Self {
        self.locked = true;
        self
    }

    /// `panel` にドッキングする（[`kernel::Parent::docked`]）。起動時の位置で追従し、グラブすると `panel` ごと移動し、
    /// `panel` を暗転しない。`panel` の閉鎖時に一緒に閉じる。
    pub fn docked_to(mut self, panel: PanelId) -> Self {
        self.from.get_or_insert(panel);
        self.parent = Some(kernel::Parent {
            panel,
            carry: true,
            docked: true,
            ..Default::default()
        });
        self
    }

    /// グラブ時に、パネル自体ではなく `anchor`（パネルから見た姿勢）を手で移動して頭部へ向ける。パネルはそれに追従する。
    pub fn grab_anchor(mut self, anchor: Pose) -> Self {
        self.grab_anchor = Some(anchor);
        self
    }

    /// グラブによるリサイズを無効化する（固定サイズのパネル。ダッシュボードのバー等）。
    pub fn without_resize(mut self) -> Self {
        self.resizable = false;
        self
    }

    /// 角を掴んで縦横の解像度（UI の表示領域）を `min`〜`max` の範囲で変えられるようにする（[`PanelCommand::CornerResize`]）。
    /// 中心は動かず、手を動かした軸だけ両側へ広がる・縮む。画面は与えられた大きさに合わせて組む。
    pub fn corner_resize(mut self, min: (u32, u32), max: (u32, u32)) -> Self {
        self.corner_resize = Some(kernel::ResizeLimits {
            min,
            max,
            keep_aspect: false,
        });
        self
    }

    /// 角を掴んで大きさを変えるとき（[`Self::corner_resize`]）、縦横比を保つ。
    pub fn keep_aspect(mut self) -> Self {
        self.keep_aspect = true;
        self
    }

    /// アタッチ中のグラブ移動範囲を、アタッチ先から `m` m 以内に制限する（[`PanelCommand::Reach`]）。
    pub fn reach(mut self, m: f32) -> Self {
        self.reach = Some(m);
        self
    }

    /// 全体設定に依存せず、このパネルでは `mode` でポインタを配送する（キーボードは即座に押下可能なようマウスモード）。
    pub fn pointer_mode(mut self, mode: PointerMode) -> Self {
        self.pointer_mode = Some(mode);
        self
    }

    /// このパネルの表示・非表示アニメーションを無効化する。
    pub fn without_animation(mut self) -> Self {
        self.animated = false;
        self
    }

    /// 表示中に空間全体を暗転する（ダッシュボード等）。暗転・復帰はパネルの表示・非表示と同じ動きで遷移する。
    pub fn with_veil(mut self) -> Self {
        self.veil = true;
        self
    }

    /// `hand` のレーザー（線またはカーソル）を描画するパネルにする。配置位置はオーバーレイ層が決定する。
    /// 操作対象パネルではないため、ナビゲーションバーは付けない。
    pub fn pointer(mut self, hand: Hand, part: Pointer) -> Self {
        self.pointer = Some((hand, part));
        self.nav_bar = false;
        self
    }

    /// ナビゲーションバーの中央に表示するタイトル（i18n キー）。
    pub fn titled(mut self, key: &'static str) -> Self {
        self.title = Some(Title::Key(key));
        self
    }

    /// ナビゲーションバーの中央に表示するタイトル（翻訳しない文字列。配信元のウィンドウ名等、実行時に決まるもの）。
    pub fn titled_text(mut self, text: impl Into<gpui::SharedString>) -> Self {
        self.title = Some(Title::Text(text.into()));
        self
    }

    /// ナビゲーションバーを無効化する（オプトアウト）。付与の可否は `crates/design/GUIDELINES.md` の
    /// 「ナビゲーションバー」に従う（常駐パネル・アタッチ使用パネル・ダイアログ等、閉じる・移動する対象でないもの）。
    pub fn without_nav_bar(mut self) -> Self {
        self.nav_bar = false;
        self
    }

    /// ナビゲーションバーの左端に OS キーボード（打鍵を OS の前面アプリへ送る仮想キーボード）のトグルを表示する。
    /// デスクトップ・ウィンドウを映すパネル用。表示中はボタンが強調色になる。
    pub fn with_os_keyboard(mut self) -> Self {
        self.os_keyboard = true;
        self
    }

    /// ナビゲーションバーの左端にドック（ダッシュボード。[`dock`]）着脱ボタンを表示する。
    /// グラブで引き抜く・近づけてリリースでドッキングも可能（[`PanelCommand::Dockable`]）。
    pub fn dockable(mut self) -> Self {
        self.dockable = true;
        self
    }

    /// ナビゲーションバーの右端を閉じるボタンの代わりに最小化ボタン（マイナス）にする。押下時に `minimize` を呼ぶ。
    /// ダッシュボードから表示する常駐パネル（閉じずに保持するもの）専用。
    pub fn minimizable(mut self, minimize: impl Fn(PanelId, &mut App) + 'static) -> Self {
        self.minimize = Some(std::rc::Rc::new(minimize));
        self
    }

    /// 閉じるボタンの隣に最小化ボタン（マイナス）を表示する。押下時に `minimize` を呼ぶ。配信元ウィンドウは最小化しない。
    pub fn source_minimizable(mut self, minimize: impl Fn(PanelId, &mut App) + 'static) -> Self {
        self.source_minimize = Some(std::rc::Rc::new(minimize));
        self
    }

    /// このパネル（PanelId `owner` で起動）に付与するナビゲーションバー。無効化されていれば `None`。
    /// 起動側（`ui::run`・testkit のキット）が持ち主の直後に開き、[`PanelCommand::NavBar`] で持ち主に付与する。
    pub fn nav_bar_spec(&self, owner: PanelId) -> Option<PanelSpec> {
        let (title, os_keyboard, dockable, minimize, source_minimize) = (
            self.title.clone(),
            self.os_keyboard,
            self.dockable,
            self.minimize.clone(),
            self.source_minimize.clone(),
        );
        self.nav_bar.then(|| PanelSpec {
            nav_bar: false,
            nav_bar_of: Some(owner),
            ..PanelSpec::root(Pose::IDENTITY, move |id, _, cx| {
                cx.default_global::<PanelControl>()
                    .nav_bars
                    .insert(owner, id);
                cx.observe_global::<PanelControl>(|bar: &mut NavBar, cx| {
                    attach::hold(bar.owner, &mut bar.held, cx)
                })
                .detach();
                NavBar {
                    id,
                    owner,
                    title,
                    os_keyboard,
                    dockable,
                    minimize,
                    source_minimize,
                    open: design::animation::Transition::rest(0.0),
                    held: false,
                }
            })
            .sized(Vec2::new(NAV_BAR_M, NAV_BAR_HEIGHT_M), NAV_BAR_PX)
        })
    }

    /// 起動時の VR 空間でのサイズ（m。等倍）と解像度（px）。[`Self::sized`] で指定した値（[`Self::over_nav_bar`] ならバーに合わせて拡縮する）。
    pub fn size<B: VrBackend>(&self, kernel: &Kernel<B>) -> (Vec2, (u32, u32)) {
        let k = self.nav_bar_scale(kernel).map_or(1., |(_, k)| k);
        (self.size * k, self.resolution)
    }

    /// このパネルがナビゲーションバーの場合、その持ち主（起動直後に [`PanelCommand::NavBar`] で適用する）。
    pub fn nav_bar_of(&self) -> Option<PanelId> {
        self.nav_bar_of
    }

    pub fn pose_for<B: VrBackend>(&self, kernel: &Kernel<B>, _next: PanelId) -> Pose {
        // サイズ・親からの位置は等倍前提で記述されている。実際のパネルは UI スケールで拡大されるため、同じ倍率を適用する。
        let scale = kernel.ui_scale();
        // このパネルの実際の高さの半分（m）。
        let half_height = self.size.y * scale / 2.;
        if let Some(center) = self.typing.map(|c| c * scale) {
            let hmd = kernel.hmd();
            let up = hmd.panel_up();
            // 頭部基準は上下（高さ・仰角）のみ。左右の向きと位置は呼び出し元に合わせ、頭部の回転には追従しない
            // （呼び出し元の面を水平化した向きで、頭部を原点に置いた座標系）。呼び出し元がない・水平配置の場合は頭部の向き。
            let caller = self.from.map(|p| kernel.panel(p).pose);
            let head = caller
                .and_then(|c| {
                    let n = c.orientation * Vec3::Z;
                    let back = (n - up * n.dot(up)).try_normalize()?;
                    let back = if back.dot(hmd.position - c.position) < 0. {
                        -back
                    } else {
                        back
                    };
                    let x = up.cross(back).normalize();
                    Some(Pose::new(
                        hmd.position,
                        Quat::from_mat3(&kernel::vr::Mat3::from_cols(x, back.cross(x), back)),
                    ))
                })
                .unwrap_or_else(|| hmd.leveled());
            let inverse = head.inverse();
            // 呼び出し元の中心の左右位置にホームポジションを合わせる。
            let lateral = caller.map_or(0., |c| inverse.transform_point(c.position).x);
            // 手（コントローラ。両手なら中間点）。高さを手に合わせ、面を少し手方向へ向ける。左右・距離は頭部の向きで決定する。
            let (sum, count) = kernel
                .controllers()
                .iter()
                .fold((Vec3::ZERO, 0.), |(sum, n), c| {
                    (sum + c.aim.position, n + 1.)
                });
            let hands = (count > 0.).then(|| sum / count);
            // 仰臥度（上方向が頭頂側に倒れているほど 1）。起立時は手の高さ、仰臥時は視界の下寄りに遷移する。
            let lying = (1. - up.dot(Vec3::Y)).clamp(0., 1.);
            let upright = hands.map_or(TYPING_AT.y, |hands| {
                ((hands - hmd.position).dot(up) + TYPING_ABOVE_HANDS)
                    .max(TYPING_LOWEST)
                    .min(-half_height - TYPING_CLEARANCE)
            });
            let mut height = upright + (-half_height - TYPING_LYING_CLEARANCE - upright) * lying;
            // 頭部基準で呼び出し元の下端より下に見えるよう、必要に応じて下げる（呼び出し元に重ねない）。呼び出し元の
            // ドック先（ダッシュボードのバー）も含め、ナビゲーションバーも下端として扱う。視認可能な範囲に制限する。
            let callers =
                std::iter::successors(self.from, |&p| kernel.panel(p).parent.map(|q| q.panel));
            let bottom = callers
                .flat_map(|p| std::iter::once(p).chain(kernel.nav_bar(p)))
                .flat_map(|p| {
                    let panel = kernel.panel(p);
                    let (w, y) = (panel.size.x / 2., -panel.size.y / 2.);
                    [-w, 0., w].map(|x| panel.pose.transform_point(Vec3::new(x, y, 0.)))
                })
                .map(|p| inverse.transform_point(p))
                .filter(|p| p.z < 0.)
                .map(|p| p.y.atan2(p.x.hypot(p.z)))
                .reduce(f32::min);
            if let Some(bottom) = bottom {
                let reach = lateral.hypot(TYPING_AT.z);
                // 見かけの高さ（の半分）は距離で変動するため、数回の反復で収束させる。真下付近では tan が発散するため 80° で制限する。
                let mut limit = height;
                for _ in 0..3 {
                    let half = half_height.atan2(reach.hypot(limit));
                    let center = (bottom - TYPING_PANEL_GAP - half).max(-80f32.to_radians());
                    limit = reach * center.tan();
                }
                height = height.min(limit).max(TYPING_LOWEST);
            }
            let at = head.transform_point(Vec3::new(lateral, height, TYPING_AT.z));
            let aim = hands.map_or(hmd.position - up * TYPING_AIM_DROP, |hands| {
                hmd.position.lerp(hands, TYPING_TOWARD_HANDS * (1. - lying))
            });
            // 仰角のみ目（と手）へ向け、左右の向きは呼び出し元のまま維持する。
            let toward = aim - at;
            let rise = toward.dot(up);
            let back = head.orientation * Vec3::Z;
            let toward = (back * (toward - up * rise).length() + up * rise).try_normalize();
            let orientation = toward
                .and_then(|z| Some((up.cross(z).try_normalize()?, z)))
                .map_or(head.orientation, |(x, z)| {
                    Quat::from_mat3(&kernel::vr::Mat3::from_cols(x, z.cross(x), z))
                });
            return Pose::new(at - orientation * Vec3::X * center, orientation);
        }
        match self.from {
            // 親の中心をアンカーに、親の手前へ少し離して重ねる（子の子はさらに手前に積層される）。
            Some(from) => {
                let offset = self.offset.unwrap_or(Pose::new(SPAWN_GAP, Quat::IDENTITY));
                let (base, k) = match self.nav_bar_scale(kernel) {
                    Some((bar, k)) => (kernel.panel(bar).pose, k),
                    None => (kernel.panel(from).pose, 1.),
                };
                base * Pose::new(offset.position * scale * k, offset.orientation)
            }
            // グラブ基準点があれば、基準点が等倍時と同じ位置に来るよう配置する（ダッシュボードは画面が立つ位置）。
            None => {
                let pose = if self.in_view {
                    kernel.hmd().leveled() * self.pose
                } else {
                    self.pose
                };
                match self.grab_anchor {
                    Some(anchor) => kernel::overlay::anchored(pose, anchor, scale),
                    None => pose,
                }
            }
        }
    }

    /// GPUI のビューを構築する。
    pub fn build(self, panel: PanelId, window: &mut Window, cx: &mut App) -> AnyView {
        (self.build)(panel, window, cx)
    }
}

/// ナビゲーションバーの内容: 持ち主のタイトルと閉じるボタン（持ち主を閉じるとバーも閉じる）。
/// 持ち主（またはバー）にレーザーがポイントされていない間は丸い線に収縮し、ポイント時に帯に展開する（[`design::components::nav_bar`] の `open`）。
/// ナビゲーションバーのタイトル。
#[derive(Clone)]
pub(crate) enum Title {
    /// i18n キー（言語切り替え時に再翻訳する）。
    Key(&'static str),
    /// そのまま表示する文字列。
    Text(gpui::SharedString),
}

/// カーネルにパネルを作成し、GPUI ウィンドウを起動する。
/// ランタイムがオーバーレイを作成できない場合（数の上限等）はエラーを出力して `None` を返す。
pub(crate) fn open_panel(
    kernel: &mut Kernel,
    mut spec: PanelSpec,
    index: usize,
    cx: &mut App,
) -> Option<(PanelId, AnyWindowHandle)> {
    let pose = spec.pose_for(kernel, index);
    let (size, resolution) = spec.size(kernel);
    let panel = match kernel.add_panel(&format!("nyaw.panel.{index}"), pose, size, resolution) {
        Ok(p) => p,
        Err(e) => {
            logger::error!("cannot open panel", error = ?e);
            return None;
        }
    };
    if !spec.animated {
        kernel.apply(panel, PanelCommand::Animated(false));
    }
    if spec.veil {
        kernel.apply(panel, PanelCommand::Veil(true));
    }
    if let Some((device, offset)) = spec.attach.take() {
        kernel.apply(panel, PanelCommand::Attach(device, offset));
    }
    if spec.locked {
        kernel.apply(panel, PanelCommand::Lock(true));
    }
    if spec.lockdown {
        kernel.apply(panel, PanelCommand::Lockdown(true));
    }
    if let Some(follow) = spec.lazy_follow {
        kernel.apply(panel, PanelCommand::LazyFollow(Some(follow)));
    }
    if !spec.resizable {
        kernel.apply(panel, PanelCommand::Resizable(false));
    }
    if let Some(limits) = spec.corner_resize {
        let keep_aspect = spec.keep_aspect;
        kernel.apply(
            panel,
            PanelCommand::CornerResize(Some(kernel::ResizeLimits {
                keep_aspect,
                ..limits
            })),
        );
    }
    if spec.dockable {
        kernel.apply(panel, PanelCommand::Dockable(true));
    }
    if spec.reach.is_some() {
        kernel.apply(panel, PanelCommand::Reach(spec.reach));
    }
    if let Some(mode) = spec.pointer_mode {
        kernel.apply(panel, PanelCommand::PointerMode(mode));
    }
    if spec.grab_anchor.is_some() {
        kernel.apply(panel, PanelCommand::GrabAnchor(spec.grab_anchor));
    }
    if let Some(parent) = spec.parent {
        kernel.apply(panel, PanelCommand::Parent(Some(parent)));
    }
    if let Some((hand, part)) = spec.pointer {
        kernel.apply(panel, PanelCommand::Pointer(hand, part));
    }
    if let Some(owner) = spec.nav_bar_of {
        kernel.apply(panel, PanelCommand::NavBar(owner));
    }
    // ナビゲーションバーは次フレームで起動する（呼び出し側が PanelId 順にウィンドウを積むため、ここでは起動しない）。
    if let Some(bar) = spec.nav_bar_spec(panel) {
        cx.default_global::<PanelControl>().spawns.push(bar);
    }
    let drag_layer = spec.pointer.is_some_and(|(_, part)| part == Pointer::Drag);
    let window: AnyWindowHandle = open_panel_window(cx, spec.resolution, |w, cx| {
        let view = spec.build(panel, w, cx);
        cx.new(|_| PanelRoot::new(view))
    })
    .expect("open panel window")
    .into();
    if drag_layer {
        cx.default_global::<DragWindows>().0.push(window);
    }
    Some((panel, window))
}

#[cfg(test)]
mod tests;
