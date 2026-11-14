//! パネルの起動仕様（[`PanelSpec`]）: 画面と、VR 空間での置き場所・大きさ・親子関係・ナビゲーションバーの有無。

use super::*;

/// 親パネルから見た、スポーンしたパネルの位置（親の中心の手前 10cm）。
const SPAWN_GAP: Vec3 = Vec3::new(0.0, 0.0, 0.1);
/// 打ちやすい位置に出すパネル（[`PanelSpec::typing`]）を向ける先を、目からどれだけ下げるか（m。コントローラが無いとき）。
const TYPING_AIM_DROP: f32 = 0.15;
/// 打ちやすい位置に出すパネルの中心を、手（コントローラ）の高さからどれだけ上げるか（m）。手を動かさず、
/// 手首を少し起こすだけで指せる高さ。
const TYPING_ABOVE_HANDS: f32 = 0.1;
/// 打ちやすい位置に出すパネルの中心の、目から見た高さの下限（m）。手を下ろしきって開いても見える所に収める。
const TYPING_LOWEST: f32 = -0.6;
/// 立っているとき、打ちやすい位置に出すパネルの上端を目の高さからどれだけ下げるか（m）。手を上げていても目線より下に出す。
const TYPING_CLEARANCE: f32 = 0.05;
/// 寝ているとき、打ちやすい位置に出すパネルの上端を視線からどれだけ下げるか（m。頭から見て）。視界には入れつつ、
/// 正面のパネルに重ねない。寝ていると手の高さは当てにならないので、手には合わせない。
const TYPING_LYING_CLEARANCE: f32 = 0.25;
/// 打ちやすい位置に出すパネルの上端と、呼び出し元（とそのナビゲーションバー・はめた先）の下端の、頭から見た角度の隙間（rad）。
/// 視界の中で呼び出し元の真下に、重ならずに見える。
const TYPING_PANEL_GAP: f32 = 3.0 * std::f32::consts::PI / 180.0;
/// 打ちやすい位置に出すパネルの面を向ける先の、目から手への寄せ具合（0 = 目、1 = 手）。読みやすさ（頭）を重く見つつ、
/// 手から来るレーザーが浅く当たらないよう手の方へも少し向ける。
const TYPING_TOWARD_HANDS: f32 = 0.3;

type PanelBuilder = dyn FnOnce(PanelId, &mut Window, &mut App) -> AnyView;

/// ナビゲーションバーのしまうボタンで呼ぶ処理（[`PanelSpec::minimizable`]）。引数はしまうパネル。
pub(crate) type Minimize = std::rc::Rc<dyn Fn(PanelId, &mut App)>;

/// 画面と VR 空間での配置。異なる画面型を同じ起動リストに入れられる。
pub struct PanelSpec {
    pose: Pose,
    from: Option<PanelId>,
    /// 開いた瞬間の頭の前の打ちやすい位置に出す（[`Self::typing`]）。値はパネルの中心から見た、呼び出し元の中心の左右に合わせる点の横位置（m）。
    pub(crate) typing: Option<f32>,
    /// `from` から見た置き場所（[`Self::offset`]）。無ければ `from` の手前に少し離して重ねる。
    offset: Option<Pose>,
    /// `pose` を開いた瞬間の頭（向きは水平方向だけ）からの相対姿勢とする。
    in_view: bool,
    pub(crate) size: Vec2,
    pub(crate) resolution: (u32, u32),
    /// 起動時に取り付ける先と相対姿勢。
    attach: Option<(Device, Pose)>,
    /// 起動時から掴めない。
    pub(crate) locked: bool,
    /// 頭にゆるく追従する（[`Self::lazy_follow`]）。
    lazy_follow: Option<kernel::LazyFollow>,
    /// 開いた時点からロックダウンを取る（[`Self::lockdown`]）。
    lockdown: bool,
    /// 掴んで大きさを変えられる（既定。[`Self::without_resize`] で外す）。
    resizable: bool,
    /// 取り付けたまま掴んで動かせる範囲（[`Self::reach`]）。
    reach: Option<f32>,
    /// 全体の設定によらないポインタの方式（[`Self::pointer_mode`]）。
    pointer_mode: Option<PointerMode>,
    /// 掴んだときに手で動かす基準（[`Self::grab_anchor`]）。
    grab_anchor: Option<Pose>,
    /// 親子関係（[`Self::child`]）。
    parent: Option<kernel::Parent>,
    animated: bool,
    /// 出ている間、空間全体を暗くする（[`Self::with_veil`]）。
    veil: bool,
    /// レーザーを描くパネル。
    pointer: Option<(Hand, Pointer)>,
    /// ナビゲーションバーに出すタイトル。
    title: Option<Title>,
    /// ナビゲーションバーを付けるか（既定は付ける。[`Self::without_nav_bar`] で外す）。
    nav_bar: bool,
    /// このパネル自身がナビゲーションバーなら、その持ち主。
    nav_bar_of: Option<PanelId>,
    /// ナビゲーションバーの左端に OS キーボードのトグルを出す（[`Self::with_os_keyboard`]）。
    os_keyboard: bool,
    /// ナビゲーションバーの左端に、はめる場所から外す・戻すボタンを出す（[`Self::dockable`]）。
    dockable: bool,
    /// ナビゲーションバーの右端を、閉じる代わりにしまうボタンにする（[`Self::minimizable`]）。
    minimize: Option<Minimize>,
    /// 閉じるボタンの隣にしまうボタンを出す（[`Self::source_minimizable`]）。
    source_minimize: Option<Minimize>,
    build: Box<PanelBuilder>,
}

/// ナビゲーションバーの窓（px）。帯（[`design::components::NAV_BAR_HEIGHT`]）の周りに影の余白を取る。
const NAV_BAR_PX: (u32, u32) = (
    480 + 2 * design::components::PANEL_MARGIN as u32,
    design::components::NAV_BAR_HEIGHT as u32 + 2 * design::components::PANEL_MARGIN as u32,
);

/// ナビゲーションバーの頭から 1m 先での幅（m）。帯の高さがツールチップの箱の 2 倍に見える縮尺にする。
/// オーバーレイ層が距離に比例して大きくするので、近くても遠くても見た目の大きさは変わらない。
const NAV_BAR_M: f32 = NAV_BAR_PX.0 as f32 * 2.0 * design::components::TOOLTIP_HEIGHT_M
    / design::components::NAV_BAR_HEIGHT;

/// ナビゲーションバーの窓（影の余白込み）の、頭から 1m 先での高さ（m）。オーバーレイ層が距離に比例して大きくする。
pub const NAV_BAR_HEIGHT_M: f32 = NAV_BAR_M * NAV_BAR_PX.1 as f32 / NAV_BAR_PX.0 as f32;

impl PanelSpec {
    /// 開いた元の無いパネル（起動時に開く常駐のもの、テストの最初のパネル）。`pose` に置く。
    /// ほかのパネルから開くものは [`Self::spawn`] / [`Self::child`] / [`Self::popover`] で作る（既定で開いた元の手前に
    /// 重なる）。別の場所に出すときだけ、[`Self::in_view`] などで明示して外す。
    pub fn root<V: Render>(
        pose: Pose,
        build: impl FnOnce(PanelId, &mut Window, &mut Context<V>) -> V + 'static,
    ) -> Self {
        Self {
            pose,
            from: None,
            typing: None,
            offset: None,
            in_view: false,
            size: Vec2::new(0.8, 0.6),
            resolution: PANEL_PX,
            attach: None,
            locked: false,
            lazy_follow: None,
            lockdown: false,
            resizable: true,
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

    /// パネル `from` から開くパネル。既定で `from` の手前に少し離して重ねる（[`Self::offset`] で変えられる）。
    pub fn spawn<V: Render>(
        from: PanelId,
        build: impl FnOnce(PanelId, &mut Window, &mut Context<V>) -> V + 'static,
    ) -> Self {
        Self {
            from: Some(from),
            ..Self::root(Pose::IDENTITY, build)
        }
    }

    /// `from` に固定したポップオーバー（メニューなど）として出す。`from` を暗くせず入力も止めず、外側を押すと閉じる。
    /// 置き場所は [`Self::offset`] で決める。ナビゲーションバーは付けない。
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

    /// 開いた瞬間の `from`（[`Self::spawn`] / [`Self::child`] / [`Self::popover`] の親）から見た置き場所。
    pub fn offset(mut self, offset: Pose) -> Self {
        self.offset = Some(offset);
        self
    }

    /// `from` の子として出す（ダイアログなど）。親に追従し、開いている間の親は暗くなって入力を受けない。
    /// 子を掴んだとき、`carry` なら親も連れていき、そうでなければ子だけが離れる。
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

    /// 子の外側を押しても閉じないようにする（既定は閉じる）。親がなければ何もしない。
    pub fn without_dismiss(mut self) -> Self {
        if let Some(p) = &mut self.parent {
            p.dismiss = false;
        }
        self
    }

    /// 親の左上に重ねる、表示だけの子にする（[`kernel::Parent::hud`]）。親がなければ何もしない。
    pub fn hud(mut self) -> Self {
        if let Some(p) = &mut self.parent {
            (p.hud, p.dismiss) = (true, false);
        }
        self
    }

    /// 親子関係を作る要求（親がいなければ `None`）。開いた直後に [`PanelCommand::Parent`] で適用する。
    pub fn parent(&self) -> Option<kernel::Parent> {
        self.parent
    }

    /// 開いた瞬間の目の前に出す。`offset` は頭から見た相対姿勢（頭の傾きは無視し、パネルは立てて置く）。
    /// トラッキング空間の原点は人の居場所と一致しないので、召喚するパネルはこれで出す。
    /// 開いた元の手前に重ねる既定（[`Self::spawn`]）から外すことになるので、外す理由を呼ぶ側に書く。
    pub fn in_view(mut self, offset: Pose) -> Self {
        (self.pose, self.from, self.in_view) = (offset, None, true);
        self
    }

    /// 大きさ（m）と解像度（px）を指定する。既定は 0.8m × 0.6m / 800 × 600。
    pub fn sized(mut self, size: Vec2, resolution: (u32, u32)) -> Self {
        (self.size, self.resolution) = (size, resolution);
        self
    }

    /// 解像度の縦横比を保ったまま、縦の大きさを `height` m にする（中身の組み方を変えずに大きさだけ揃える）。
    pub fn fit_height(mut self, height: f32) -> Self {
        self.size = Vec2::new(
            height * self.resolution.0 as f32 / self.resolution.1 as f32,
            height,
        );
        self
    }

    /// 開いた瞬間の頭から手を軽く前に出したあたり（打ちやすい位置）に出す。高さと仰ぐ角度だけを頭（と手）から決め、
    /// 左右の向きと位置は呼び出し元（[`Self::spawn`] の `from`）に合わせる。`center` はパネルの中心から見た横位置（m）で、
    /// そこを呼び出し元の中心の左右に合わせる（キーボードならホームポジション）。親子関係や追従は作らない。
    pub fn typing(mut self, center: f32) -> Self {
        self.typing = Some(center);
        self
    }

    /// 起動時にデバイスへ取り付ける。
    pub fn attached(mut self, device: Device, offset: Pose) -> Self {
        self.attach = Some((device, offset));
        self
    }

    /// 開いた時点からロックダウンを取る（[`PanelCommand::Lockdown`]）。放す（か閉じる）まで、このパネルは閉じられず、
    /// ダッシュボードなどのシェルも開閉を控える。放すのはビューが `Lockdown(false)` を積む。
    pub fn lockdown(mut self) -> Self {
        self.lockdown = true;
        self
    }

    /// 頭の向き（左右だけ）と歩きにゆるく追従する（[`kernel::LazyFollow`]）。ほかのパネルでも使える。
    pub fn lazy_follow(mut self, follow: kernel::LazyFollow) -> Self {
        self.lazy_follow = Some(follow);
        self
    }

    /// 起動時から掴めなくする。
    pub fn locked(mut self) -> Self {
        self.locked = true;
        self
    }

    /// `panel` に固定する（[`kernel::Parent::docked`]）。開いた時の位置のまま付いて動き、掴むと `panel` ごと動き、
    /// `panel` を暗くしない。`panel` が閉じれば一緒に閉じる。
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

    /// 掴んだとき、パネルそのものではなく `anchor`（このパネルから見た姿勢）を手で動かして頭へ向ける。パネルはそこに付いてくる。
    pub fn grab_anchor(mut self, anchor: Pose) -> Self {
        self.grab_anchor = Some(anchor);
        self
    }

    /// 掴んでも大きさを変えられないようにする（大きさの決まったもの。ダッシュボードのバーなど）。
    pub fn without_resize(mut self) -> Self {
        self.resizable = false;
        self
    }

    /// 取り付けたまま掴んで動かせるのを、取り付け先から `m` m 以内に限る（[`PanelCommand::Reach`]）。
    pub fn reach(mut self, m: f32) -> Self {
        self.reach = Some(m);
        self
    }

    /// 全体の設定によらず、このパネルでは `mode` でポインタを流す（キーボードは押した瞬間に押せるようマウス）。
    pub fn pointer_mode(mut self, mode: PointerMode) -> Self {
        self.pointer_mode = Some(mode);
        self
    }

    /// このパネルのイン・アウトアニメーションを無効にする。
    pub fn without_animation(mut self) -> Self {
        self.animated = false;
        self
    }

    /// 出ている間、空間全体を暗くする（ダッシュボードなど）。暗転・復帰はパネルの出入りと同じ動きで進む。
    pub fn with_veil(mut self) -> Self {
        self.veil = true;
        self
    }

    /// `hand` のレーザー（線またはカーソル）を描くパネルにする。置き場所はオーバーレイ層が決める。
    /// 操作するパネルではないので、ナビゲーションバーは付けない。
    pub fn pointer(mut self, hand: Hand, part: Pointer) -> Self {
        self.pointer = Some((hand, part));
        self.nav_bar = false;
        self
    }

    /// ナビゲーションバーの中央に出すタイトル（i18n のキー）。
    pub fn titled(mut self, key: &'static str) -> Self {
        self.title = Some(Title::Key(key));
        self
    }

    /// ナビゲーションバーの中央に出すタイトル（訳さない文字列。配信元のウィンドウ名など、実行時に決まるもの）。
    pub fn titled_text(mut self, text: impl Into<gpui::SharedString>) -> Self {
        self.title = Some(Title::Text(text.into()));
        self
    }

    /// ナビゲーションバーを付けない（オプトアウト）。付けてよいかは `crates/design/GUIDELINES.md` の
    /// 「ナビゲーションバー」に従う（常駐・取り付けて使うパネルやダイアログなど、閉じる・動かす対象でないもの）。
    pub fn without_nav_bar(mut self) -> Self {
        self.nav_bar = false;
        self
    }

    /// ナビゲーションバーの左端に、OS キーボード（打った文字を OS の前面のアプリへ送る仮想キーボード）のトグルを出す。
    /// デスクトップ・ウィンドウを映すパネル用。出ている間はボタンが強調色になる。
    pub fn with_os_keyboard(mut self) -> Self {
        self.os_keyboard = true;
        self
    }

    /// ナビゲーションバーの左端に、はめる場所（ダッシュボード。[`dock`]）から外して独立させる・戻すボタンを出す。
    pub fn dockable(mut self) -> Self {
        self.dockable = true;
        self
    }

    /// ナビゲーションバーの右端を、閉じる代わりにしまうボタン（マイナス）にする。押すと `minimize` を呼ぶ。
    /// ダッシュボードから出す常駐のパネル（閉じずに置いておくもの）だけに使う。
    pub fn minimizable(mut self, minimize: impl Fn(PanelId, &mut App) + 'static) -> Self {
        self.minimize = Some(std::rc::Rc::new(minimize));
        self
    }

    /// 閉じるボタンの隣に、しまうボタン（マイナス）を出す。押すと `minimize` を呼ぶ。配信元ウィンドウは最小化しない。
    pub fn source_minimizable(mut self, minimize: impl Fn(PanelId, &mut App) + 'static) -> Self {
        self.source_minimize = Some(std::rc::Rc::new(minimize));
        self
    }

    /// このパネル（PanelId `owner` で開いた）に付けるナビゲーションバー。外していれば `None`。
    /// 開く側（`ui::run`・testkit のキット）が持ち主の直後に開き、[`PanelCommand::NavBar`] で持ち主に付ける。
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
                NavBar {
                    id,
                    owner,
                    title,
                    os_keyboard,
                    dockable,
                    minimize,
                    source_minimize,
                    open: design::animation::Transition::rest(0.0),
                }
            })
            .sized(Vec2::new(NAV_BAR_M, NAV_BAR_HEIGHT_M), NAV_BAR_PX)
        })
    }

    /// VR 上の大きさ（m。等倍）と解像度（px）。[`Self::sized`] で決めたもの。
    pub fn size(&self) -> (Vec2, (u32, u32)) {
        (self.size, self.resolution)
    }

    /// このパネルがナビゲーションバーなら、その持ち主（開いた直後に [`PanelCommand::NavBar`] で適用する）。
    pub fn nav_bar_of(&self) -> Option<PanelId> {
        self.nav_bar_of
    }

    pub fn pose_for<B: VrBackend>(&self, kernel: &Kernel<B>, _next: PanelId) -> Pose {
        // 大きさ・開いた元からの位置は等倍のつもりで書いてある。実際のパネルは UI スケールで大きいので、同じだけ掛ける。
        let scale = kernel.ui_scale();
        // このパネルの実際の高さの半分。
        let half_height = self.size.y * scale / 2.;
        if let Some(center) = self.typing.map(|c| c * scale) {
            let hmd = kernel.hmd();
            let up = hmd.panel_up();
            // 頭から見るのは上下（高さ・仰ぐ角度）だけ。左右の向きと位置は呼び出し元に合わせ、頭の回転に付いて行かない
            // （呼び出し元の面を水平にした向きで、頭を原点に置いた座標）。呼び出し元が無い・寝かせてあれば頭の向き。
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
            // 呼び出し元の中心の左右の位置に、ホームポジションを合わせる。
            let lateral = caller.map_or(0., |c| inverse.transform_point(c.position).x);
            // 手（コントローラ。両手なら間）。高さを手に合わせ、面を少し手へ向ける。左右・距離は頭の向きで決める。
            let (sum, count) = kernel
                .controllers()
                .iter()
                .fold((Vec3::ZERO, 0.), |(sum, n), c| {
                    (sum + c.aim.position, n + 1.)
                });
            let hands = (count > 0.).then(|| sum / count);
            // 寝ている（上が頭の上へ倒れている）ほど 1。立っている間は手の高さ、寝ている間は視界の下寄りへ移る。
            let lying = (1. - up.dot(Vec3::Y)).clamp(0., 1.);
            let upright = hands.map_or(TYPING_AT.y, |hands| {
                ((hands - hmd.position).dot(up) + TYPING_ABOVE_HANDS)
                    .max(TYPING_LOWEST)
                    .min(-half_height - TYPING_CLEARANCE)
            });
            let mut height = upright + (-half_height - TYPING_LYING_CLEARANCE - upright) * lying;
            // 頭から見て呼び出し元の下端より下に見えるよう、必要なら下げる（呼び出し元の邪魔をしない）。呼び出し元を
            // はめた先（ダッシュボードのバー）も含め、ナビゲーションバーの分も下端とみなす。下げても見える所までに収める。
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
                // 自分の見かけの高さ（の半分）は距離で変わるので、数回詰める。真下近くでは tan が発散するので 80° で止める。
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
            // 仰ぐ角度だけ目（と手）へ向け、左右の向きは呼び出し元のまま。
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
            // 親の中心をアンカーに、親の手前へ少しだけ離して重ねる（子の子はさらに手前へ積まれる）。
            Some(from) => {
                let offset = self.offset.unwrap_or(Pose::new(SPAWN_GAP, Quat::IDENTITY));
                kernel.panel(from).pose * Pose::new(offset.position * scale, offset.orientation)
            }
            // 掴む基準があれば、基準が等倍のときと同じ所に来るように置く（ダッシュボードは画面が立つ所）。
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

    /// GPUI のビューを組み立てる。
    pub fn build(self, panel: PanelId, window: &mut Window, cx: &mut App) -> AnyView {
        (self.build)(panel, window, cx)
    }
}

/// ナビゲーションバーの中身: 持ち主のタイトルと閉じるボタン（持ち主を閉じ、バーも一緒に閉じる）。
/// 持ち主（かバー）をレーザーが指していない間は丸い線に縮み、指すと帯に広がる（[`design::components::nav_bar`] の `open`）。
/// ナビゲーションバーのタイトル。
#[derive(Clone)]
pub(crate) enum Title {
    /// i18n のキー（言語を切り替えると訳し直す）。
    Key(&'static str),
    /// そのまま出す文字列。
    Text(gpui::SharedString),
}

/// カーネルにパネルを作り、GPUI のウィンドウを開く。
/// ランタイムがオーバーレイを作れない（数の上限など）ときはエラーを出して `None`。
pub(crate) fn open_panel(
    kernel: &mut Kernel,
    mut spec: PanelSpec,
    index: usize,
    cx: &mut App,
) -> Option<(PanelId, AnyWindowHandle)> {
    let pose = spec.pose_for(kernel, index);
    let panel = match kernel.add_panel(
        &format!("nyaw.panel.{index}"),
        pose,
        spec.size,
        spec.resolution,
    ) {
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
    // ナビゲーションバーは次のフレームで開く（呼び出し側が PanelId の順に窓を積むため、ここでは開かない）。
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
