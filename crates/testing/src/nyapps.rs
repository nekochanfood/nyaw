use super::*;
use gpui::AnyElement;

/// ニャプリの一覧と詳細（`nyapp-test`）。左にシステムのニャプリと入っているニャプリ、右に選んだものの詳細
/// （ID・種類・版・ABI・権限・更新情報・パッケージ・アイコン・起動中のパネル）。
pub struct NyappTest {
    panel: PanelId,
    /// 入っているニャプリ（読み直している間は `None`）。
    installed: Option<Vec<nyapp::Installed>>,
    /// 選んでいるもの（システムの ID か、入っているものの ID）。
    pub selected: Option<String>,
}

impl NyappTest {
    pub fn new(panel: PanelId, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            panel,
            installed: None,
            selected: Some(nyapp::system::WRIST.id.to_owned()),
        };
        this.scan(cx);
        this
    }

    /// 入っているニャプリを読み直す（パッケージを読むので別スレッド）。
    fn scan(&mut self, cx: &mut Context<Self>) {
        self.installed = None;
        let panel = self.panel;
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async { nyapp::installed::scan() })
                .await;
            let _ = this.update(cx, |t, cx| {
                t.installed = Some(found);
                redraw(panel, cx);
            });
        })
        .detach();
    }

    /// 左の一覧の 1 行（アイコンと名前。選んでいる行は強調色）。
    fn row(
        &self,
        theme: &Theme,
        id: &str,
        icon: AnyElement,
        name: SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.selected.as_deref() == Some(id);
        let owned = id.to_owned();
        components::content_button(
            theme,
            SharedString::from(format!("nyapp-row-{id}")),
            if selected {
                Variant::Primary
            } else {
                Variant::Ghost
            },
        )
        .w_full()
        .justify_start()
        .child(icon)
        .child(div().min_w_0().truncate().child(name))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.selected = Some(owned.clone());
            redraw(this.panel, cx);
        }))
        .into_any_element()
    }

    /// 詳細の項目（名前・値）。
    fn details(&self, cx: &App) -> Vec<(SharedString, String)> {
        let Some(id) = self.selected.as_deref() else {
            return Vec::new();
        };
        let label = |key| i18n::t(cx, key);
        let running = nyapp::panel_of(id, cx).map_or_else(
            || i18n::t(cx, "nyapp_test.none").to_string(),
            |p| p.to_string(),
        );
        if let Some(system) = nyapp::system::ALL.iter().find(|s| s.id == id) {
            return vec![
                (label("nyapp_test.id"), system.id.to_owned()),
                (
                    label("nyapp_test.name"),
                    i18n::t(cx, system.name).to_string(),
                ),
                (
                    label("nyapp_test.kind"),
                    i18n::t(cx, "nyapp_test.system").to_string(),
                ),
                (
                    label("nyapp_test.version"),
                    env!("CARGO_PKG_VERSION").to_owned(),
                ),
                (
                    label("nyapp_test.permissions"),
                    i18n::t(cx, "nyapp_test.all").to_string(),
                ),
                (
                    label("nyapp_test.package"),
                    i18n::t(cx, "nyapp_test.builtin").to_string(),
                ),
                (label("nyapp_test.running"), running),
            ];
        }
        let Some(app) = self
            .installed
            .iter()
            .flatten()
            .find(|a| a.manifest.id == id)
        else {
            return Vec::new();
        };
        let m = &app.manifest;
        let permissions = if m.permissions.is_empty() {
            i18n::t(cx, "nyapp_test.none").to_string()
        } else {
            m.permissions
                .iter()
                .map(|p| format!("{p:?}").to_lowercase())
                .collect::<Vec<_>>()
                .join(", ")
        };
        vec![
            (label("nyapp_test.id"), m.id.clone()),
            (
                label("nyapp_test.name"),
                m.name_in(i18n::Lang::current(cx).code()).to_owned(),
            ),
            (
                label("nyapp_test.kind"),
                i18n::t(cx, "nyapp_test.user").to_string(),
            ),
            (label("nyapp_test.version"), m.version.clone()),
            (label("nyapp_test.api"), m.api.to_string()),
            (label("nyapp_test.permissions"), permissions),
            (
                label("nyapp_test.update"),
                m.update.as_ref().map_or_else(
                    || i18n::t(cx, "nyapp_test.none").to_string(),
                    |u| u.url.clone(),
                ),
            ),
            (
                label("nyapp_test.icon"),
                i18n::t(
                    cx,
                    if app.icon.is_packaged() {
                        "nyapp_test.icon_package"
                    } else {
                        "nyapp_test.icon_fallback"
                    },
                )
                .to_string(),
            ),
            (label("nyapp_test.package"), app.path.display().to_string()),
            (label("nyapp_test.running"), running),
        ]
    }
}

/// 一覧の行のアイコンの大きさ（px）と、詳細の上のアイコンの大きさ。
const ROW_ICON_PX: f32 = 28.0;
const DETAIL_ICON_PX: f32 = 64.0;

impl Render for NyappTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let lang = i18n::Lang::current(cx).code();
        let system_icon = || {
            design::icon(&design::icons::GEAR_SIX)
                .size(px(ROW_ICON_PX))
                .into_any_element()
        };
        let mut rows = vec![section_label(&theme, i18n::t(cx, "nyapp_test.system"))];
        for s in nyapp::system::ALL {
            let row = self.row(&theme, s.id, system_icon(), i18n::t(cx, s.name), cx);
            rows.push(row);
        }
        rows.push(section_label(&theme, i18n::t(cx, "nyapp_test.user")));
        match self.installed.clone() {
            None => rows.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(spacing::SM))
                    .px(px(spacing::SM))
                    .pt(px(spacing::SM))
                    .text_color(rgba(theme.neutral_text_low))
                    .child(components::spinner(&theme, 16.0))
                    .child(i18n::t(cx, "dashboard.loading"))
                    .into_any_element(),
            ),
            Some(apps) if apps.is_empty() => {
                rows.push(section_label(&theme, i18n::t(cx, "nyapp_test.none")))
            }
            Some(apps) => {
                for app in apps {
                    let icon = app.icon(lang, ROW_ICON_PX, &theme);
                    let name = SharedString::from(app.manifest.name_in(lang).to_owned());
                    let row = self.row(&theme, &app.manifest.id, icon, name, cx);
                    rows.push(row);
                }
            }
        }
        let selected_icon = self.selected.as_deref().and_then(|id| {
            if nyapp::system::is_system(id) {
                return Some(
                    design::icon(&design::icons::GEAR_SIX)
                        .size(px(DETAIL_ICON_PX))
                        .into_any_element(),
                );
            }
            let app = self
                .installed
                .iter()
                .flatten()
                .find(|a| a.manifest.id == id)?;
            Some(app.icon(lang, DETAIL_ICON_PX, &theme))
        });
        let details = self.details(cx).into_iter().map(|(k, v)| {
            div()
                .flex()
                .gap(px(spacing::MD))
                .child(
                    div()
                        .w(px(140.0))
                        .flex_none()
                        .text_color(rgba(theme.neutral_text_low))
                        .child(k),
                )
                .child(div().min_w_0().flex_1().child(v))
        });
        components::panel(&theme).child(
            div()
                .size_full()
                .flex()
                .gap(px(spacing::MD))
                .child(
                    div()
                        .id("nyapp-list")
                        .w(px(280.0))
                        .flex_none()
                        .flex()
                        .flex_col()
                        .gap(px(spacing::XS))
                        .overflow_y_scroll()
                        .children(rows)
                        .child(
                            components::button(
                                &theme,
                                "nyapp-rescan",
                                i18n::t(cx, "dashboard.refresh"),
                                Variant::Default,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.scan(cx);
                                redraw(this.panel, cx);
                            })),
                        ),
                )
                .child(
                    components::card(&theme)
                        .id("nyapp-detail")
                        .debug_selector(|| "nyapp-detail".into())
                        .flex_1()
                        .min_w_0()
                        .p(px(spacing::LG))
                        .flex()
                        .flex_col()
                        .gap(px(spacing::SM))
                        .overflow_y_scroll()
                        .children(selected_icon)
                        .children(details),
                ),
        )
    }
}

fn section_label(theme: &Theme, text: SharedString) -> AnyElement {
    div()
        .px(px(spacing::SM))
        .pt(px(spacing::SM))
        .text_size(px(font_size::SM))
        .font_weight(FontWeight::BOLD)
        .text_color(rgba(theme.neutral_text_low))
        .child(text)
        .into_any_element()
}

fn redraw(panel: PanelId, cx: &mut App) {
    cx.default_global::<PanelControl>()
        .panels
        .entry(panel)
        .or_default()
        .redraw = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    /// システムのニャプリはすべて並び、選ぶとその詳細（ID・種類）が出る。入っているものは別スレッドで読む。
    #[gpui::test]
    fn lists_system_nyapps_and_shows_details(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, cx| NyappTest::new(1, cx));
        cx.run_until_parked();
        for s in nyapp::system::ALL {
            assert!(
                cx.debug_bounds(Box::leak(format!("nyapp-row-{}", s.id).into_boxed_str()))
                    .is_some(),
                "{}",
                s.id
            );
        }
        let at = cx
            .debug_bounds(Box::leak(
                format!("nyapp-row-{}", nyapp::system::LAUNCHER.id).into_boxed_str(),
            ))
            .unwrap()
            .center();
        cx.simulate_click(at, gpui::Modifiers::none());
        let details = view.update(cx, |v, cx| {
            assert_eq!(v.selected.as_deref(), Some(nyapp::system::LAUNCHER.id));
            v.details(cx)
        });
        assert!(details.iter().any(|(_, v)| v == nyapp::system::LAUNCHER.id));
        assert!(
            view.read_with(cx, |v, _| v.installed.is_some()),
            "入っているものを読み終える"
        );
    }
}
