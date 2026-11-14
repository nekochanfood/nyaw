//! Layer 5 画面: 設定画面。GNOME の設定のようなツートーン: 左のサイドバー（地は `sidebar`）にカテゴリの一覧、
//! 右（地は `background`）に選んだカテゴリの設定。中身はカテゴリ・詳しい画面ごとのファイルに分ける
//! （[`general`] / [`appearance`] / [`input`] / [`display`] / [`pointer`] / [`keyboard`] / [`update`] / [`about`]）。
//! このファイルは画面の骨組み（サイドバー・積み重ね・入れ替え）と、どのカテゴリでも使う部品（選択の行・詳しい画面を開く行）だけを持つ。

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
/// 一覧の行（`components::slider_item`）に置くスライダーの長さ（px）。
const ROW_SLIDER_PX: f32 = 240.0;
/// 画面の入れ替えで動く距離（画面の幅・高さに対する割合）。全幅は動かず、透明度の入れ替えで見せる。
const PAGE_SHIFT: f32 = 0.5;

/// 色は `design` のダークテーマの役割から引く（`color_at` は 0xRRGGBB なのでアルファを落とす）。
const THEME: Theme = Theme::of(Mode::Dark);
/// サイドバーの地。行は選んでいてもいなくても同じ色（背景が無いように見える）。
pub const SIDEBAR_BG: u32 = THEME.sidebar >> 8;
/// サイドバーで選んでいる行の左端の印の色。
pub const SIDEBAR_INDICATOR: u32 = THEME.accent_solid >> 8;
pub use design::components::SIDEBAR_INDICATOR_INSET;

/// 設定のカテゴリ（サイドバー。並びはこの順）。段階的開示: カテゴリはよく変える設定だけを直接並べ、
/// まとまった設定は詳しい画面（[`Page`]）に分けて行から開く。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    /// 一般（アップデートはここから開く。ダッシュボードのボタンからも）。
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

    /// サイドバーの行の `debug_selector`・アイコン・文言のキー。
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

/// カテゴリの中から開く詳しい画面（サブカテゴリ）。積み重ね（[`components::navigation`]）に右から積み、
/// 戻る（`settings-back`）かスワイプバックで下ろす。サイドバーでカテゴリを選ぶと全部下ろす。
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
    /// 情報 → ライセンスの一覧。
    Licenses,
}

impl Page {
    /// 開く行の `debug_selector`・アイコン・見出しの文言のキー。
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

/// 設定画面の起動仕様（ダッシュボードの設定ボタン。main が渡す）。`opened` は作られたパネルの ID を受け取る。
pub fn spec(from: PanelId, opened: impl FnOnce(PanelId, &mut gpui::App) + 'static) -> PanelSpec {
    PanelSpec::spawn(from, |id, _, cx| {
        opened(id, cx);
        Settings::new(id)
    })
    .sized(SIZE, PX)
    .titled("testing.open.settings")
}

/// 設定画面。サイドバーの行（`components::sidebar_item`）は選んでいるものだけ文字が濃く、左端に印が付く。
/// 中身はカードを縦に並べる。複数から選ぶ設定は選択の行（`components::select`）で、押すと選択肢のポップオーバーが開き、
/// 選ぶとすぐ全体の設定（`Setting`）に反映する。外観と時計は `ui::prefs` の部品。
/// まとまった設定は詳しい画面（[`Page`]）で、カテゴリの中の行（`components::list_item`）から積んで開く。
#[derive(Default)]
pub struct Settings {
    /// このパネル（色を作るダイアログを子として開く）。
    panel: PanelId,
    /// 選んでいるカテゴリ。
    pub category: Category,
    /// カテゴリの上に積んだ詳しい画面（先頭が下）。空ならカテゴリの中身を見せる。
    pub pages: Vec<Page>,
    /// 外観・時計の部品の状態。
    pub appearance: ui::prefs::Appearance,
    pub watch: ui::prefs::WatchFaces,
    /// キーボードの見本（最初にキーボードの画面を開いたときに作り、画面を閉じるまで持つ）。
    pub keyboard_preview: std::cell::OnceCell<gpui::Entity<ui::keyboard::KeyboardView>>,
    /// 選択肢を開いている選択の行（`debug_selector`）。
    pub open: Option<&'static str>,
    /// スクロール（スクロールバーが見る）: カテゴリの中身、詳しい画面（積むたびに先頭から）、ライセンスの一覧。
    /// 積み下ろしの間は 2 枚を同時に描くので、カテゴリと詳しい画面で分ける。
    scroll: ScrollHandle,
    page_scroll: ScrollHandle,
    licenses_scroll: gpui::UniformListScrollHandle,
    /// カテゴリを替えた直後の入れ替え: 前のカテゴリと、その進み具合（0 → 1）。
    swap: Option<(Category, design::animation::Transition)>,
}

impl Settings {
    pub fn new(panel: PanelId) -> Self {
        Self {
            panel,
            ..Self::default()
        }
    }

    /// 詳しい画面 `page` を積む。時計は入るたびに、見ていた文字盤と絞り込みを忘れて使っている文字盤を中央に戻す
    /// （出ていく画面は滑り去る間も描くので、出るときではなく入るときに戻す）。ライセンスの一覧の下の情報は、
    /// 戻ったときに元の位置のままにする（積むのが情報の上なので、詳しい画面のスクロールは先頭に戻さない）。
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

    /// 押すと詳しい画面 `page` を積む行（右に `>`）。
    fn page_row(&self, theme: &Theme, page: Page, cx: &mut Context<Self>) -> components::Button {
        let (id, glyph, key) = page.row();
        components::list_item(theme, id, glyph, i18n::t(cx, key)).on_click(cx.listener(
            move |this, _, _, cx| {
                this.push(page);
                cx.notify();
            },
        ))
    }

    /// 詳しい画面を開く行のカード。
    fn page_rows<const N: usize>(
        &self,
        theme: &Theme,
        pages: [Page; N],
        cx: &mut Context<Self>,
    ) -> Div {
        components::list(theme, pages.map(|p| self.page_row(theme, p, cx)))
    }

    /// 選択の行（`components::select`）。押すと選択肢のポップオーバーを開き、選ぶと `set` で全体の設定に反映して閉じる。
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
        // 閉じる動きのために、閉じていても毎回渡す（消えきると何も描かない）。
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

    /// カテゴリの中身（グループの並び）。よく変える設定を直接並べ、まとまった設定は詳しい画面を開く行にする。
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
            // 文字の見本・文字の大きさ・UI スケール・太字（セットアップウィザードと同じ部品）。
            Category::Accessibility => vec![ui::prefs::accessibility(window, cx)],
        }
    }

    /// 詳しい画面の中身（グループの並び）。
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

/// 押すと全体の設定の変更を積むクリックハンドラ。
fn change(s: Setting) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| ui::set(s, cx)
}

/// 見出しの行。サイドバーと中身で高さを揃え、一覧の上端を揃える。タイトルは左右の中央、`left` のボタン（戻る）は左端に寄せる（閉じるのはナビゲーションバー）。
/// 高さはアイコンボタンと同じなので、ボタンは上下にも左右にも余白を足さない（パネルの内側の余白だけが四方に揃う）。
/// 積み重ねを試す画面（`testing::NavTest`）も同じ見出しを使う。
pub fn heading(title: impl IntoElement, left: Option<gpui::AnyElement>) -> Div {
    // ボタンの有無で中央がずれないよう、左右にボタンと同じ幅の枠を常に取り、タイトルはその間の中央に置く（長ければ省略）。
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
        // ダッシュボードのアップデートのボタンから開いた（開いていた）なら、アップデートを出す。
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
                    // 選んだカテゴリの中身に戻る（今のカテゴリでも、積んだ詳しい画面を下ろす）。
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
        // 画面の積み重ね: 一番下がカテゴリの中身（`None`）、その上に詳しい画面を積むと右から入る。
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
        // パネルの地は中身側（`background`）。サイドバーだけ `sidebar` で塗り、左の角をパネルに合わせて丸める。
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
                    // 中身の幅で広がらず、パネルの残りに収める（長い文字は行の中で省略する）。
                    .min_w_0()
                    .child(
                        components::navigation(&theme, "settings-nav", &stack, page, swipe_back)
                            // ページの地がパネルの右の角丸を覆わない。
                            .rounded_right(radius::XL)
                            .fading(PAGE_SHIFT),
                    ),
            )
    }
}

impl Settings {
    /// 積み重ねの画面 1 枚: 見出しの下にカテゴリの中身（`None`）、または詳しい画面。
    /// カテゴリを替えた直後は、前のカテゴリの画面と上下にスライドして入れ替わる（サイドバーで下の行を選ぶと上へ、上の行なら下へ）。
    fn page(&self, page: Option<Page>, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let swap = self.swap.filter(|_| page.is_none());
        if let Some((from, tr)) = swap {
            let (t, moving) = tr.at(design::drive::now());
            if moving {
                components::keep_drawing(window, cx);
                let dir = if self.category > from { 1.0 } else { -1.0 };
                let old = self.page_of(None, from, true, window, cx);
                let new = self.page_of(None, self.category, false, window, cx);
                // 出る側も入る側も、動きは小さく、透明度を入れ替える。
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

    /// 画面 1 枚。`leaving` は入れ替えで抜けていく側（スクロールの追跡とスクロールバーは持たない）。
    /// 詳しい画面は見出しの左に戻る（`settings-back`）を置く。ライセンスの一覧は自分でスクロールする。
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
