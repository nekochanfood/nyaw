//! 再生中: ジャケットをぼかした背景に、アプリのボタン・ジャケット・タイトル・アーティスト・シークバー・再生の操作。

use super::*;

/// 再生中の画面（背景に大きな画像が載る）の Ghost ボタンの、ホバー・押下の背景の不透明度。
const NOW_PLAYING_BACKGROUND_OPACITY: f32 = 0.125;
/// 再生中の画面: ジャケットの一辺、シークバーの幅・棒の太さ・押せる高さ（px）、背景に重ねる地の濃さ（0..=255）。
const ART_DISPLAY_PX: f32 = 280.0;
const SEEK_PX: f32 = 560.0;
const SEEK_BAR_PX: f32 = 6.0;
const SEEK_HIT_PX: f32 = 32.0;
const NOW_PLAYING_VEIL: u32 = 0x8c;

/// 再生中の画面の画像。`key` は元の画像（[`kernel::subsystems::media::Image`]）の中身の場所。
pub(crate) struct MediaImages {
    key: [usize; 3],
    art: Option<Arc<RenderImage>>,
    backdrop: Option<Arc<RenderImage>>,
    icon: Option<Arc<RenderImage>>,
}

impl Screen {
    /// 再生中のメディアのアプリのウィンドウ（取得元の一覧から。実行ファイル名か、ウィンドウの名前で探す）。
    fn app_window(&self, app: &media::App) -> Option<&(Source, Option<Arc<RenderImage>>)> {
        let (id, name) = (app_key(&app.id), app.name.to_lowercase());
        let exe = app.exe.as_deref().map(app_key);
        self.sources.iter().find(|(s, _)| {
            s.kind == SourceKind::Window
                && (s.app.as_deref().is_some_and(|a| {
                    let a = app_key(a);
                    a == id || a == name || exe.as_ref() == Some(&a)
                }) || (!name.is_empty() && s.name.to_lowercase().contains(&name)))
        })
    }

    pub(crate) fn now_playing(&mut self, cx: &mut Context<Self>) -> Div {
        self.poll_sources();
        let Some(m) = cx.global::<PanelControl>().media.clone() else {
            let theme = Theme::current(cx);
            return components::card_panel(&theme)
                .flex()
                .items_center()
                .justify_center()
                .text_color(rgba(theme.neutral_text_low))
                .child(i18n::t(cx, "dashboard.nothing_playing"));
        };
        // アプリのウィンドウを映す。無ければ、対応するプレイヤーだけに前面化を頼む。
        if self.open_app && self.pending.is_none() {
            self.open_app = false;
            let window = self.app_window(&m.app);
            match window
                .map(|(source, icon)| (source, icon, mirror_of(source, cx).map(|(id, _)| id)))
            {
                // もう映していれば新しく開かない。ダッシュボードに隠している配信なら、それを出す。
                Some((_, _, Some(id))) => {
                    let _ = self.dashboard.update(cx, |d, cx| d.show_stream(id, cx));
                }
                Some((source, icon, None)) => {
                    let docked = ui::docked(cx) == Some(self.panel);
                    let spec = mirror_spec(
                        source.clone(),
                        icon.clone(),
                        Some(self.dashboard.clone()),
                        self.panel,
                        false,
                        docked,
                    );
                    cx.default_global::<PanelControl>().spawns.push(spec);
                }
                None if m.controls.raise => {
                    commands(self.panel, [PanelCommand::Media(media::Command::Raise)], cx)
                }
                None => {}
            }
        }
        let place = |i: &Option<media::Image>| i.as_ref().map_or(0, |i| i.rgba.as_ptr() as usize);
        let key = [place(&m.art), place(&m.backdrop), place(&m.app.icon)];
        if self.media_images.as_ref().is_none_or(|i| i.key != key) {
            let convert = |i: &Option<media::Image>| {
                i.as_ref()
                    .and_then(|i| ui::render_image(i.width, i.height, i.rgba.to_vec()))
            };
            self.media_images = Some(MediaImages {
                key,
                art: convert(&m.art),
                backdrop: convert(&m.backdrop),
                icon: convert(&m.app.icon),
            });
        }
        let images = self.media_images.as_ref().expect("made above");
        // 背景がジャケットで変わるので、明るい背景には暗い文字（ライト）、暗い背景には明るい文字（ダーク）を載せる。
        let theme = match m.light {
            Some(true) => Theme::LIGHT,
            Some(false) => Theme::DARK,
            None => Theme::current(cx),
        };
        let low = theme.neutral_text_low;
        // Ghost ボタンの背景は、背景に紛れないよう反対のテーマの色にする（明るい背景には暗く、暗い背景には明るく）。
        let contrast = if theme == Theme::LIGHT {
            Theme::DARK
        } else {
            Theme::LIGHT
        };
        let app_window = self.app_window(&m.app);
        let can_open_app = app_window.is_some() || m.controls.raise;
        let icon = images
            .icon
            .clone()
            .or_else(|| app_window.and_then(|(_, i)| i.clone()));
        let panel = self.panel;
        let send = move |command| {
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                commands(panel, [PanelCommand::Media(command)], cx)
            }
        };
        let control = |id: &'static str, glyph, key, enabled: bool, command| {
            components::icon_button(&theme, id, glyph, i18n::t(cx, key), Variant::Ghost)
                .ghost_background(&contrast)
                .background_opacity(NOW_PLAYING_BACKGROUND_OPACITY)
                .when(!enabled, |b| b.opacity(0.4))
                .when(enabled, |b| b.on_click(send(command)))
        };
        let (position, duration) = (m.position_now(), m.duration);
        let fraction = duration.map_or(0.0, |d| {
            (position.as_secs_f32() / d.as_secs_f32().max(0.001)).clamp(0.0, 1.0)
        });
        let seek_bounds = std::rc::Rc::new(std::cell::Cell::new(gpui::Bounds::default()));
        let bounds_in = seek_bounds.clone();
        components::card_panel(&theme)
            .when_some(images.backdrop.clone(), |d, backdrop| {
                d.child(
                    img(backdrop)
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(gpui::ObjectFit::Cover)
                        .rounded(px(design::radius::XL)),
                )
            })
            // ぼかしたジャケットの上に地の色を薄く重ね、文字と部品を読みやすくする。
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(design::radius::XL))
                    .bg(rgba(theme.background & !0xff | NOW_PLAYING_VEIL)),
            )
            .child(
                div()
                    .absolute()
                    .top(px(spacing::LG))
                    .left(px(spacing::LG))
                    .child(
                        components::content_button(&theme, "now-playing-app", Variant::Ghost)
                            .ghost_background(&contrast)
                            .background_opacity(NOW_PLAYING_BACKGROUND_OPACITY)
                            .tooltip(i18n::t(
                                cx,
                                if can_open_app {
                                    "dashboard.open_app"
                                } else {
                                    "dashboard.app_unavailable"
                                },
                            ))
                            .when(!can_open_app, |b| b.opacity(0.4))
                            .child(match icon {
                                Some(icon) => img(icon).size(px(font_size::LG)).into_any_element(),
                                None => design::icon(&icons::APP_WINDOW)
                                    .size(px(font_size::LG))
                                    .into_any_element(),
                            })
                            .child(m.app.name.clone())
                            .when(can_open_app, |b| {
                                b.on_click(cx.listener(|this, _, _, cx| {
                                    this.open_app = true;
                                    this.refresh();
                                    ui::notify(this.panel, cx);
                                }))
                            }),
                    ),
            )
            // メディアが複数あるときだけ、右上に前後のメディアへの切り替えを出す。
            .when(m.sessions > 1, |d| {
                d.child(
                    div()
                        .absolute()
                        .top(px(spacing::LG))
                        .right(px(spacing::LG))
                        .flex()
                        .gap(px(spacing::SM))
                        .child(control(
                            "now-playing-previous-media",
                            &icons::CARET_LEFT,
                            "dashboard.previous_media",
                            true,
                            media::Command::Switch(false),
                        ))
                        .child(control(
                            "now-playing-next-media",
                            &icons::CARET_RIGHT,
                            "dashboard.next_media",
                            true,
                            media::Command::Switch(true),
                        )),
                )
            })
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(spacing::SM))
                    .text_color(rgba(theme.neutral_text))
                    .child(match images.art.clone() {
                        Some(art) => img(art)
                            .size(px(ART_DISPLAY_PX))
                            .rounded(px(design::radius::LG))
                            .into_any_element(),
                        None => div()
                            .size(px(ART_DISPLAY_PX))
                            .rounded(px(design::radius::LG))
                            .bg(rgba(theme.control))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(design::icon(&icons::MUSIC_NOTES).size(px(TILE_ICON_PX)))
                            .into_any_element(),
                    })
                    .child(
                        div()
                            .debug_selector(|| "now-playing-title".into())
                            .mt(px(spacing::MD))
                            .max_w(px(SEEK_PX))
                            .truncate()
                            .text_size(px(font_size::LG))
                            .font_weight(FontWeight::BOLD)
                            .child(m.title.clone()),
                    )
                    .child(
                        div()
                            .max_w(px(SEEK_PX))
                            .truncate()
                            .text_color(rgba(low))
                            .child(m.artist.clone()),
                    )
                    // 1 行に、再生位置・シークバー・曲の長さ。
                    .child(
                        div()
                            .mt(px(spacing::MD))
                            .w(px(SEEK_PX))
                            .flex()
                            .items_center()
                            .gap(px(spacing::SM))
                            .text_size(px(font_size::SM))
                            .text_color(rgba(low))
                            .child(
                                div()
                                    .flex_none()
                                    .debug_selector(|| "now-playing-position".into())
                                    .child(clock(position)),
                            )
                            // シークバー: ハンドルの無い無彩色の棒。押した位置へ移る（シークできるときだけ）。
                            .child(
                                div()
                                    .id("now-playing-seek")
                                    .debug_selector(|| "now-playing-seek".into())
                                    .flex_1()
                                    .min_w_0()
                                    .h(px(SEEK_HIT_PX))
                                    .relative()
                                    .flex()
                                    .items_center()
                                    .child(
                                        gpui::canvas(
                                            move |bounds, _, _| bounds_in.set(bounds),
                                            |_, _, _, _| {},
                                        )
                                        .absolute()
                                        .inset_0(),
                                    )
                                    .child(
                                        div()
                                            .w_full()
                                            .h(px(SEEK_BAR_PX))
                                            .rounded(px(design::radius::FULL))
                                            .bg(rgba(theme.neutral_text_low & !0xff | 0x55))
                                            .child(
                                                div()
                                                    .h_full()
                                                    .w(gpui::relative(fraction))
                                                    .rounded(px(design::radius::FULL))
                                                    .bg(rgba(theme.neutral_text)),
                                            ),
                                    )
                                    .when_some(
                                        duration.filter(|_| m.controls.seek),
                                        |d, duration| {
                                            d.on_mouse_down(
                                                gpui::MouseButton::Left,
                                                move |e, _, cx| {
                                                    let b = seek_bounds.get();
                                                    let at = f32::from(e.position.x - b.origin.x)
                                                        / f32::from(b.size.width).max(1.0);
                                                    let to = duration.mul_f32(at.clamp(0.0, 1.0));
                                                    commands(
                                                        panel,
                                                        [PanelCommand::Media(
                                                            media::Command::Seek(to),
                                                        )],
                                                        cx,
                                                    );
                                                },
                                            )
                                        },
                                    ),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .debug_selector(|| "now-playing-duration".into())
                                    .child(duration.map_or_else(|| "--:--".to_owned(), clock)),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(spacing::SM))
                            .flex()
                            .gap(px(spacing::LG))
                            .child(control(
                                "now-playing-previous",
                                &icons::SKIP_BACK,
                                "dashboard.previous",
                                m.controls.previous,
                                media::Command::Previous,
                            ))
                            .child(control(
                                "now-playing-play-pause",
                                if m.playing {
                                    &icons::PAUSE
                                } else {
                                    &icons::PLAY
                                },
                                if m.playing {
                                    "dashboard.pause"
                                } else {
                                    "dashboard.play"
                                },
                                m.controls.play_pause,
                                media::Command::PlayPause,
                            ))
                            .child(control(
                                "now-playing-next",
                                &icons::SKIP_FORWARD,
                                "dashboard.next",
                                m.controls.next,
                                media::Command::Next,
                            )),
                    ),
            )
    }
}

#[cfg(test)]
mod tests;
