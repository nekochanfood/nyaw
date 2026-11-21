//! Layer 5 画面: 設定画面。GNOME 設定風のツートーン構成で、左サイドバー（背景色 `sidebar`）にカテゴリ一覧、
//! 右側（背景色 `background`）に選択カテゴリの設定を表示する。内容はカテゴリ・詳細画面ごとにファイル分割する
//! （[`general`] / [`appearance`] / [`input`] / [`display`] / [`pointer`] / [`keyboard`] / [`update`] / [`about`]）。
//! 本ファイルは画面の骨格（サイドバー・ナビゲーションスタック・遷移アニメーション）と共通部品（選択行・詳細画面遷移行）のみを持つ。

use design::components::{self, Variant};
use design::{Mode, Theme, font_size, radius, spacing};
use gpui::{
    App, Context, Div, ScrollHandle, SharedString, Window, div, prelude::*, px, relative, rgba,
};
use kernel::vr::Vec2;
use kernel::{PanelId, Setting};
use ui::PanelSpec;

mod about;
mod appearance;
mod display;
mod general;
mod input;
mod keyboard;
mod pointer;
mod update;

/// 設定画面の大きさ（m）と解像度（px）。1px = 1mm。
pub const SIZE: Vec2 = Vec2::new(1.024, 0.768);
pub const PX: (u32, u32) = (1024, 768);
/// サイドバーの幅（px）。
const SIDEBAR_PX: f32 = 320.0;
/// リスト行（`components::slider_item`）内に配置するスライダーの長さ（px）。
const ROW_SLIDER_PX: f32 = 240.0;
/// 画面遷移時のスライド距離（画面サイズに対する割合）。全幅遷移は行わず、スライド＋フェードで遷移を表現する。
const PAGE_SHIFT: f32 = 0.5;

/// 色は `design` のダークテーマのセマンティックトークンから取得する（`color_at` は 0xRRGGBB のためアルファを除去）。
const THEME: Theme = Theme::of(Mode::Dark);
/// サイドバー背景色。行は選択状態に関わらず同一色（背景が透過して見える）。
pub const SIDEBAR_BG: u32 = THEME.sidebar >> 8;
/// サイドバーの選択行左端インジケーターの色。
pub const SIDEBAR_INDICATOR: u32 = THEME.accent_solid >> 8;
pub use design::components::SIDEBAR_INDICATOR_INSET;

/// 設定のカテゴリ（サイドバーに表示、定義順に並ぶ）。段階的開示の原則に従い、頻用する設定のみ直接表示し、
/// 関連設定群は詳細画面（[`Page`]）に分離してリスト行から遷移する。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    /// 一般（アップデートはここから遷移。ダッシュボードのボタンからも直接遷移可能）。
    #[default]
    General,
    Appearance,
    Input,
    Display,
    Accessibility,
}

impl Category {
    pub const ALL: [Self; 5] = [
        Self::General,
        Self::Appearance,
        Self::Input,
        Self::Display,
        Self::Accessibility,
    ];

    /// サイドバー行の `debug_selector`・アイコン・i18n キー。
    fn row(self) -> (&'static str, &'static design::Icon, &'static str) {
        match self {
            Self::General => (
                "settings-general",
                &design::icons::GEAR,
                "settings.category.general",
            ),
            Self::Appearance => (
                "settings-appearance",
                &design::icons::PALETTE,
                "settings.category.appearance",
            ),
            Self::Input => (
                "settings-input",
                &design::icons::GAME_CONTROLLER,
                "settings.category.input",
            ),
            Self::Display => (
                "settings-display",
                &design::icons::MONITOR,
                "settings.category.display",
            ),
            Self::Accessibility => (
                "settings-accessibility",
                &design::icons::PERSON_ARMS_SPREAD,
                "settings.category.accessibility",
            ),
        }
    }
}

/// カテゴリ内の詳細画面（サブカテゴリ）。ナビゲーションスタック（[`components::navigation`]）に右から積み、
/// 戻るボタン（`settings-back`）またはスワイプバックでポップする。サイドバーでカテゴリを選択すると全画面をポップする。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    /// 外観 → 時計の文字盤。
    Watch,
    /// 入力 → ポインタ（カーソルとビーム）。
    Pointer,
    /// 入力 → キーボード。
    Keyboard,
    /// 一般 → アップデート。
    Update,
    /// 一般 → 情報。
    About,
    /// 情報 → ライセンス一覧。
    Licenses,
}

impl Page {
    /// 遷移行の `debug_selector`・アイコン・見出しの i18n キー。
    fn row(self) -> (&'static str, &'static design::Icon, &'static str) {
        match self {
            Self::Watch => (
                "settings-watch",
                &design::icons::WATCH,
                "settings.category.watch",
            ),
            Self::Pointer => (
                "settings-pointer",
                &design::icons::CURSOR,
                "settings.category.pointer",
            ),
            Self::Keyboard => (
                "settings-keyboard",
                &design::icons::KEYBOARD,
                "settings.category.keyboard",
            ),
            Self::Update => (
                "settings-update",
                &design::icons::ARROW_CIRCLE_UP,
                "settings.category.update",
            ),
            Self::About => (
                "settings-about",
                &design::icons::INFO,
                "settings.category.about",
            ),
            Self::Licenses => (
                "setting-licenses",
                &design::icons::SCROLL,
                "settings.licenses",
            ),
        }
    }
}

/// 設定画面の起動仕様（ダッシュボードの設定ボタン用、main が渡す）。`opened` は生成されたパネル ID を受け取るコールバック。
pub fn spec(from: PanelId, opened: impl FnOnce(PanelId, &mut gpui::App) + 'static) -> PanelSpec {
    PanelSpec::spawn(from, |id, _, cx| {
        opened(id, cx);
        Settings::new(id)
    })
    .sized(SIZE, PX)
    .titled("testing.open.settings")
}

/// 設定画面。サイドバー行（`components::sidebar_item`）は選択中のもののみテキストが濃く、左端にインジケーターが付く。
/// 本体はカードを縦に配置する。選択式設定は `components::select` 行で、クリックでポップオーバーを開き、
/// 選択するとグローバル設定（`Setting`）に即時反映する。外観と時計は `ui::prefs` の共通部品を使用。
/// 関連設定群は詳細画面（[`Page`]）として、カテゴリ内のリスト行（`components::list_item`）からスタックに積んで遷移する。
#[derive(Default)]
pub struct Settings {
    /// このパネルの ID（カラーピッカーダイアログを子パネルとして開く際に使用）。
    panel: PanelId,
    /// 現在選択中のカテゴリ。
    pub category: Category,
    /// カテゴリ上に積んだ詳細画面のスタック（先頭が底）。空の場合はカテゴリ本体を表示する。
    pub pages: Vec<Page>,
    /// 外観・時計の共通部品の状態。
    pub appearance: ui::prefs::Appearance,
    pub watch: ui::prefs::WatchFaces,
    /// キーボードプレビュー（キーボード画面の初回遷移時に生成し、画面が閉じるまで保持する）。
    pub keyboard_preview: std::cell::OnceCell<gpui::Entity<ui::keyboard::KeyboardView>>,
    /// ポップオーバーが開いている選択行の `debug_selector`。
    pub open: Option<&'static str>,
    /// スクロールハンドル（スクロールバー用）: カテゴリ本体、詳細画面（プッシュごとに先頭にリセット）、ライセンス一覧。
    /// スタック遷移中は 2 画面を同時に描画するため、カテゴリと詳細画面でハンドルを分離する。
    scroll: ScrollHandle,
    page_scroll: ScrollHandle,
    licenses_scroll: gpui::UniformListScrollHandle,
    /// カテゴリ切替直後の遷移アニメーション: 切替前のカテゴリと進捗（0→1）。
    swap: Option<(Category, design::animation::Transition)>,
}

impl Settings {
    pub fn new(panel: PanelId) -> Self {
        Self {
            panel,
            ..Self::default()
        }
    }

    /// 詳細画面 `page` をスタックに積む。時計画面は遷移のたびに文字盤選択とフィルターをリセットし、使用中の文字盤を中央に戻す
    /// （退出画面はスライドアウト中も描画されるため、退出時ではなく進入時にリセットする）。ライセンス一覧下の情報画面は、
    /// 戻った際にスクロール位置を維持する（ライセンスが情報の上に積まれるため、詳細画面のスクロールはリセットしない）。
    pub fn push(&mut self, page: Page) {
        if page == Page::Watch {
            self.watch = Default::default();
        }
        if page != Page::Licenses {
            self.page_scroll = ScrollHandle::new();
        }
        self.pages.push(page);
        self.open = None;
    }

    /// クリックで詳細画面 `page` をスタックに積む行（右端に `>` アイコン）。
    fn page_row(&self, theme: &Theme, page: Page, cx: &mut Context<Self>) -> components::Button {
        let (id, glyph, key) = page.row();
        components::list_item(theme, id, glyph, i18n::t(cx, key)).on_click(cx.listener(
            move |this, _, _, cx| {
                this.push(page);
                cx.notify();
            },
        ))
    }

    /// 詳細画面への遷移行をまとめたカード。
    fn page_rows<const N: usize>(
        &self,
        theme: &Theme,
        pages: [Page; N],
        cx: &mut Context<Self>,
    ) -> Div {
        components::list(theme, pages.map(|p| self.page_row(theme, p, cx)))
    }

    /// 選択行（`components::select`）。クリックでポップオーバーを開き、選択すると `set` でグローバル設定に反映して閉じる。
    #[allow(clippy::too_many_arguments)]
    fn select<T: Copy + PartialEq + 'static, I: Into<SharedString>>(
        &self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
        id: &'static str,
        glyph: &'static design::Icon,
        label: SharedString,
        now: T,
        options: Vec<(I, SharedString, T)>,
        set: fn(T) -> Setting,
    ) -> components::Button {
        let value = options
            .iter()
            .find(|(_, _, v)| *v == now)
            .map(|(_, l, _)| l.clone())
            .unwrap_or_default();
        // 閉じるアニメーションのため、非表示状態でも毎フレーム渡す（完全に消えると描画しない）。
        let items = options
            .into_iter()
            .map(|(oid, l, v)| {
                components::select_option(theme, oid, l, v == now).on_click(cx.listener(
                    move |this, _, _, cx| {
                        ui::set(set(v), cx);
                        this.open = None;
                        cx.notify();
                    },
                ))
            })
            .collect::<Vec<_>>();
        let dismiss = cx.listener(|this, _, _, cx| {
            this.open = None;
            cx.notify();
        });
        let popover = components::select_popover(
            theme,
            window,
            cx,
            id,
            self.open == Some(id),
            items,
            dismiss,
        );
        components::select(theme, id, glyph, label, value)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open = Some(id);
                cx.notify();
            }))
            .child(popover)
    }

    /// カテゴリ本体（グループの並び）。頻用する設定を直接配置し、関連設定群は詳細画面への遷移行とする。
    fn category_page(
        &self,
        theme: &Theme,
        category: Category,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        match category {
            Category::General => general::page(self, theme, window, cx),
            Category::Appearance => appearance::page(self, theme, window, cx),
            Category::Input => input::page(self, theme, window, cx),
            Category::Display => display::page(self, theme, window, cx),
            // テキストプレビュー・フォントサイズ・UI スケール・太字（セットアップウィザードと共通部品）。
            Category::Accessibility => vec![ui::prefs::accessibility(window, cx)],
        }
    }

    /// 詳細画面の本体（グループの並び）。
    fn sub_page(
        &self,
        theme: &Theme,
        page: Page,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        match page {
            Page::Watch => appearance::watch(self, window, cx),
            Page::Pointer => pointer::page(self, theme, window, cx),
            Page::Keyboard => keyboard::page(self, theme, window, cx),
            Page::Update => update::page(self, theme, window, cx),
            Page::About => about::page(self, theme, cx),
            Page::Licenses => about::licenses(self, theme),
        }
    }
}

/// クリックでグローバル設定の変更を適用するハンドラ。
fn change(s: Setting) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| ui::set(s, cx)
}

/// 見出し行。サイドバーと本体で高さを揃え、リスト上端を整列させる。タイトルは水平中央配置、`left` のボタン（戻る）は左端に寄せる（閉じるはナビゲーションバーが担当）。
/// 高さはアイコンボタンと同一のため、ボタンには上下左右の追加余白を設けない（パネル内側の余白のみで四方が揃う）。
/// ナビゲーションスタックのテスト画面（`testing::NavTest`）も同一の見出しを使用する。
pub fn heading(title: impl IntoElement, left: Option<gpui::AnyElement>) -> Div {
    // ボタンの有無でタイトル中央がずれないよう、左右にボタンと同幅のスロットを常時確保し、タイトルはその間に中央配置する（長い場合は省略表示）。
    let slot = || div().w(px(components::BUTTON_HEIGHT)).flex_none().flex();
    div()
        .h(px(components::BUTTON_HEIGHT))
        .flex()
        .items_center()
        .text_size(px(font_size::LG))
        .font_weight(gpui::FontWeight::BOLD)
        .child(slot().children(left))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .justify_center()
                .child(div().min_w_0().truncate().child(title)),
        )
        .child(slot())
}

impl Render for Settings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // ダッシュボードのアップデートボタンから開いた（または開いていた）場合、アップデート画面を表示する。
        if updater::take_reveal(cx) {
            (self.category, self.swap) = (Category::General, None);
            self.pages.clear();
            self.push(Page::Update);
        }
        let theme = Theme::current(cx);
        let categories = Category::ALL
            .into_iter()
            .map(|category| {
                let (id, glyph, key) = category.row();
                components::sidebar_item(
                    &theme,
                    id,
                    glyph,
                    i18n::t(cx, key),
                    category == self.category,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    // 選択したカテゴリの本体に戻る（現在のカテゴリでも、積まれた詳細画面をすべてポップする）。
                    if category != this.category {
                        let mut t = design::animation::Transition::rest(0.0);
                        let animated = cx.has_global::<design::Motion>();
                        let now = design::drive::now();
                        t.go(1.0, design::animation::Motion::Page, now, animated);
                        this.swap = Some((this.category, t));
                    }
                    this.category = category;
                    this.open = None;
                    this.appearance = Default::default();
                    this.watch.open = false;
                    this.pages.clear();
                    cx.notify();
                }))
            })
            .collect::<Vec<_>>();
        // ナビゲーションスタック: 最下層がカテゴリ本体（`None`）、その上に詳細画面を積むと右からスライドインする。
        let this = cx.entity();
        let stack: Vec<Option<Page>> = std::iter::once(None)
            .chain(self.pages.iter().copied().map(Some))
            .collect();
        let page = {
            let this = this.clone();
            move |&page: &Option<Page>, window: &mut Window, cx: &mut App| {
                this.update(cx, |s, cx| s.page(page, window, cx))
                    .into_any_element()
            }
        };
        let swipe_back = move |_: &mut Window, cx: &mut App| {
            this.update(cx, |s, cx| {
                s.pages.pop();
                s.open = None;
                cx.notify();
            })
        };
        // パネル背景は本体側（`background`）。サイドバーのみ `sidebar` で塗り、左角をパネルの角丸に合わせる。
        components::panel(&theme)
            .p(px(0.0))
            .flex()
            .child(
                div()
                    .w(px(SIDEBAR_PX))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap(px(spacing::MD))
                    .p(px(spacing::MD))
                    .rounded_l(px(radius::XL))
                    .bg(rgba(theme.sidebar))
                    .child(heading(i18n::t(cx, "settings.title"), None))
                    .child(components::sidebar(
                        &theme,
                        window,
                        cx,
                        "settings-sidebar",
                        self.category as usize,
                        categories,
                    )),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    // 本体の内容幅で膨張せず、パネルの残り幅に収める（長いテキストは行内で省略表示する）。
                    .min_w_0()
                    .child(
                        components::navigation(&theme, "settings-nav", &stack, page, swipe_back)
                            // ページ背景がパネル右側の角丸を覆わないようにする。
                            .rounded_right(radius::XL)
                            .fading(PAGE_SHIFT),
                    ),
            )
    }
}

impl Settings {
    /// スタック内の 1 画面: 見出し下にカテゴリ本体（`None`）または詳細画面を表示する。
    /// カテゴリ切替直後は前カテゴリの画面と上下スライドで遷移する（サイドバーで下の行を選ぶと上へ、上の行なら下へ）。
    fn page(&self, page: Option<Page>, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let swap = self.swap.filter(|_| page.is_none());
        if let Some((from, tr)) = swap {
            let (t, moving) = tr.at(design::drive::now());
            if moving {
                components::keep_drawing(window, cx);
                let dir = if self.category > from { 1.0 } else { -1.0 };
                let old = self.page_of(None, from, true, window, cx);
                let new = self.page_of(None, self.category, false, window, cx);
                // 退出側・進入側ともスライド量は小さく、透明度のクロスフェードで遷移を表現する。
                let slot = |top: f32, alpha: f32, page: Div| {
                    div()
                        .absolute()
                        .size_full()
                        .top(relative(top * PAGE_SHIFT))
                        .opacity(alpha)
                        .child(page)
                };
                return div()
                    .relative()
                    .size_full()
                    .overflow_hidden()
                    .child(slot(-dir * t, 1.0 - t, old))
                    .child(slot(dir * (1.0 - t), t, new));
            }
        }
        self.page_of(page, self.category, false, window, cx)
    }

    /// 画面 1 枚の描画。`leaving` は遷移アニメーションで退出中の側（スクロール追跡とスクロールバーを持たない）。
    /// 詳細画面は見出しの左に戻るボタン（`settings-back`）を配置する。ライセンス一覧は独自のスクロールを持つ。
    fn page_of(
        &self,
        page: Option<Page>,
        category: Category,
        leaving: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = Theme::current(cx);
        let (title, back) = match page {
            Some(p) => {
                let back = components::icon_button(
                    &theme,
                    "settings-back",
                    &design::icons::CARET_LEFT,
                    i18n::t(cx, "common.back"),
                    Variant::OnBackground,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.pages.pop();
                    this.open = None;
                    cx.notify();
                }));
                (p.row().2, Some(back.into_any_element()))
            }
            None => (category.row().2, None),
        };
        let licenses = page == Some(Page::Licenses);
        let content = div()
            .id(if licenses {
                "settings-licenses"
            } else if leaving {
                "settings-leaving"
            } else {
                "settings-content"
            })
            .size_full()
            .flex()
            .flex_col()
            .gap(px(spacing::LG))
            .p(px(spacing::MD))
            .pb(px(spacing::XL))
            .overflow_y_scroll()
            .child(heading(i18n::t(cx, title), back))
            .children(match page {
                Some(p) => self.sub_page(&theme, p, window, cx),
                None => self.category_page(&theme, category, window, cx),
            });
        let frame = div().relative().size_full();
        if licenses || leaving {
            return frame.child(content);
        }
        let scroll = if page.is_some() {
            &self.page_scroll
        } else {
            &self.scroll
        };
        frame
            .child(
                content
                    .debug_selector(|| "settings-content".into())
                    .track_scroll(scroll),
            )
            .child(
                components::scrollbars(&theme, "settings-bars", scroll).corner_radius(radius::XL),
            )
    }
}
