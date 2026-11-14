//! Markdown を読める形で描く（リリースノートなど）。GitHub の書き方（表・打ち消し線・チェックリスト・裸の URL）まで。
//! 解析は pulldown-cmark に任せ、ここはブロック（段落・見出し・一覧・引用・コード・表・区切り）を部品の色と大きさで並べる。
//! HTML は描かない（コメントなどは消える）。画像は代わりの文字を、画像へのリンクとして出す。

use std::ops::Range;

use gpui::{
    AnyElement, App, Bounds, Context, Div, ElementId, FontStyle, FontWeight, HighlightStyle,
    InteractiveText, Pixels, SharedString, StrikethroughStyle, StyleRefinement, StyledText,
    UnderlineStyle, Window, canvas, div, prelude::*, px, rgba,
};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::{Theme, font, font_size, radius, spacing};

/// 文字の飾り。
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct Marks {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    link: bool,
}

/// 飾りの付いた文字のかたまり（段落・見出し・表のセル）。飾りの範囲は重ならず、前から並ぶ。
#[derive(Default, Debug, PartialEq, Eq)]
struct Inline {
    text: String,
    marks: Vec<(Range<usize>, Marks)>,
    links: Vec<(Range<usize>, String)>,
}

#[derive(Debug, PartialEq, Eq)]
enum Block {
    Paragraph(Inline),
    Heading(u8, Inline),
    Code(String),
    /// 番号付きなら最初の番号。
    List(Option<u64>, Vec<Item>),
    Quote(Vec<Block>),
    /// 先頭の行が見出し。
    Table(Vec<Vec<Inline>>),
    Rule,
}

#[derive(Debug, PartialEq, Eq)]
struct Item {
    /// チェックリストの印（無ければ `None`）。
    checked: Option<bool>,
    blocks: Vec<Block>,
}

/// 組み立て途中の入れ物。
enum Open {
    Blocks(Vec<Block>),
    Quote(Vec<Block>),
    List(Option<u64>, Vec<Item>),
    Item(Option<bool>, Vec<Block>),
    Table(Vec<Vec<Inline>>),
    Row(Vec<Inline>),
}

#[derive(Default)]
struct Builder {
    open: Vec<Open>,
    inline: Option<Inline>,
    marks: Marks,
    link: Option<(usize, String)>,
    code: Option<String>,
}

impl Builder {
    fn push(&mut self, block: Block) {
        if let Some(Open::Blocks(b) | Open::Quote(b) | Open::Item(_, b)) = self.open.last_mut() {
            b.push(block);
        }
    }

    /// 書きかけの文字を段落にする（緩くない一覧の項目は、段落に包まれずに文字が来る）。
    fn flush(&mut self) {
        if let Some(i) = self.inline.take().filter(|i| !i.text.trim().is_empty()) {
            self.push(Block::Paragraph(i));
        }
    }

    fn text(&mut self, t: &str, marks: Marks) {
        let inline = self.inline.get_or_insert_default();
        let start = inline.text.len();
        inline.text.push_str(t);
        if marks != Marks::default() {
            inline.marks.push((start..inline.text.len(), marks));
        }
    }

    /// 文字を足す。リンクの外の裸の URL（GitHub が作るリリースノートにある）はリンクにする。
    fn plain(&mut self, mut t: &str) {
        let marks = self.marks;
        if self.link.is_some() {
            return self.text(t, marks);
        }
        while let Some(at) = [t.find("https://"), t.find("http://")]
            .into_iter()
            .flatten()
            .min()
        {
            let len = t[at..]
                .find(|c: char| c.is_whitespace() || "<>\"".contains(c))
                .unwrap_or(t.len() - at);
            let url = t[at..at + len].trim_end_matches(['.', ',', ':', ';', '!', '?', '\'', ')']);
            self.text(&t[..at], marks);
            let start = self.inline.get_or_insert_default().text.len();
            self.text(
                url,
                Marks {
                    link: true,
                    ..marks
                },
            );
            let inline = self.inline.as_mut().expect("just written");
            inline
                .links
                .push((start..inline.text.len(), url.to_owned()));
            t = &t[at + url.len()..];
        }
        self.text(t, marks);
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => match &mut self.code {
                Some(code) => code.push_str(&t),
                None => self.plain(&t),
            },
            Event::Code(t) => self.text(
                &t,
                Marks {
                    code: true,
                    ..self.marks
                },
            ),
            Event::SoftBreak => self.text(" ", self.marks),
            Event::HardBreak => self.text("\n", self.marks),
            Event::Rule => {
                self.flush();
                self.push(Block::Rule);
            }
            Event::TaskListMarker(done) => {
                if let Some(Open::Item(checked, _)) = self.open.last_mut() {
                    *checked = Some(done);
                }
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph | Tag::Heading { .. } => {
                self.flush();
                self.inline = Some(Inline::default());
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.open.push(Open::Quote(Vec::new()));
            }
            Tag::CodeBlock(_) => {
                self.flush();
                self.code = Some(String::new());
            }
            Tag::List(start) => {
                self.flush();
                self.open.push(Open::List(start, Vec::new()));
            }
            Tag::Item => {
                self.flush();
                self.open.push(Open::Item(None, Vec::new()));
            }
            Tag::Table(_) => {
                self.flush();
                self.open.push(Open::Table(Vec::new()));
            }
            Tag::TableHead | Tag::TableRow => self.open.push(Open::Row(Vec::new())),
            Tag::TableCell => self.inline = Some(Inline::default()),
            Tag::Emphasis => self.marks.italic = true,
            Tag::Strong => self.marks.bold = true,
            Tag::Strikethrough => self.marks.strike = true,
            Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                let at = self.inline.get_or_insert_default().text.len();
                self.link = Some((at, dest_url.into_string()));
                self.marks.link = true;
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.flush(),
            TagEnd::Heading(level) => {
                let inline = self.inline.take().unwrap_or_default();
                self.push(Block::Heading(level as u8, inline));
            }
            TagEnd::CodeBlock => {
                let code = self.code.take().unwrap_or_default();
                self.push(Block::Code(code.trim_end_matches('\n').to_owned()));
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                if let Some(Open::Quote(b)) = self.open.pop() {
                    self.push(Block::Quote(b));
                }
            }
            TagEnd::List(_) => {
                self.flush();
                if let Some(Open::List(start, items)) = self.open.pop() {
                    self.push(Block::List(start, items));
                }
            }
            TagEnd::Item => {
                self.flush();
                if let Some(Open::Item(checked, blocks)) = self.open.pop()
                    && let Some(Open::List(_, items)) = self.open.last_mut()
                {
                    items.push(Item { checked, blocks });
                }
            }
            TagEnd::TableCell => {
                let cell = self.inline.take().unwrap_or_default();
                if let Some(Open::Row(row)) = self.open.last_mut() {
                    row.push(cell);
                }
            }
            TagEnd::TableHead | TagEnd::TableRow => {
                if let Some(Open::Row(row)) = self.open.pop()
                    && let Some(Open::Table(rows)) = self.open.last_mut()
                {
                    rows.push(row);
                }
            }
            TagEnd::Table => {
                if let Some(Open::Table(rows)) = self.open.pop() {
                    self.push(Block::Table(rows));
                }
            }
            TagEnd::Emphasis => self.marks.italic = false,
            TagEnd::Strong => self.marks.bold = false,
            TagEnd::Strikethrough => self.marks.strike = false,
            TagEnd::Link | TagEnd::Image => {
                self.marks.link = false;
                if let (Some((start, url)), Some(inline)) = (self.link.take(), &mut self.inline) {
                    let end = inline.text.len();
                    if end > start {
                        inline.links.push((start..end, url));
                    }
                }
            }
            _ => {}
        }
    }
}

fn parse(source: &str) -> Vec<Block> {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut b = Builder {
        open: vec![Open::Blocks(Vec::new())],
        ..Default::default()
    };
    for event in Parser::new_ext(source, options) {
        b.event(event);
    }
    b.flush();
    match b.open.into_iter().next() {
        Some(Open::Blocks(blocks)) => blocks,
        _ => Vec::new(),
    }
}

/// 描いた Markdown。解析した結果と、前に描いたときの中身の高さを覚えておく。
struct Markdown {
    id: SharedString,
    source: String,
    blocks: Vec<Block>,
    theme: Theme,
    /// 分かるまでは使い回さずに描いて測る（使い回す包みは中身から高さを測れず、高さを先に決める必要がある）。
    height: Option<Pixels>,
}

impl Render for Markdown {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        let measure = move |bounds: Bounds<Pixels>, _: &mut Window, cx: &mut App| {
            let _ = this.update(cx, |m, _| m.height = Some(bounds.size.height));
        };
        blocks(&self.theme, &self.id, &self.blocks)
            .w_full()
            .child(canvas(measure, |_, _, _, _| {}).absolute().size_full())
    }
}

/// `source` の Markdown を縦に並べる。リンクは押すとブラウザで開く。`id` は覚えておく状態とリンクを持つ文字の要素の ID の頭。
/// 解析した結果と描いた絵は `source`・`theme`・置き場所が変わるまで使い回す（落としている間など毎フレーム描き直す画面向け）。
pub fn markdown(
    theme: &Theme,
    id: impl Into<SharedString>,
    source: &str,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let id = id.into();
    let view = window.use_keyed_state(id.clone(), cx, |_, _| Markdown {
        id,
        source: source.to_owned(),
        blocks: parse(source),
        theme: *theme,
        height: None,
    });
    view.update(cx, |m, cx| {
        if m.source != source {
            m.source = source.to_owned();
            m.blocks = parse(source);
            m.height = None;
            cx.notify();
        }
        if m.theme != *theme {
            m.theme = *theme;
            cx.notify();
        }
    });
    // ponytail: 幅が変わったフレームは前の高さのまま描き、測り直した高さは次に描くときに効く。ずれが目立つなら測ってすぐ描き直す。
    match view.read(cx).height {
        Some(h) => view
            .cached(StyleRefinement::default().w_full().h(h))
            .into_any_element(),
        None => view.into_any_element(),
    }
}

fn blocks(theme: &Theme, id: &str, list: &[Block]) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(spacing::MD))
        .children(list.iter().enumerate().map(|(i, b)| {
            let id = format!("{id}-{i}");
            block(theme, &id, b).debug_selector(|| id)
        }))
}

/// ブロック 1 つ。中のブロックの `debug_selector` は `{id}-{添字}`（一覧の項目の中は `{id}-{項目}-{添字}`、印は `{id}-{項目}-marker`）。
fn block(theme: &Theme, id: &str, b: &Block) -> Div {
    let low = rgba(theme.neutral_text_low);
    match b {
        Block::Paragraph(i) => div().child(inline(theme, id, i)),
        Block::Heading(level, i) => div()
            .font_weight(FontWeight::BOLD)
            .text_size(px(if *level <= 2 {
                font_size::LG
            } else {
                font_size::MD
            }))
            .child(inline(theme, id, i)),
        Block::Code(code) => div()
            .p(px(spacing::MD))
            .rounded(px(radius::MD))
            .bg(rgba(theme.control))
            .font_family(font::MONO_FAMILY)
            .text_size(px(font_size::SM))
            .child(code.clone()),
        Block::List(start, items) => {
            div()
                .flex()
                .flex_col()
                .gap(px(spacing::XS))
                .children(items.iter().enumerate().map(|(n, item)| {
                    let marker = match (item.checked, start) {
                        (Some(done), _) => crate::icon(if done {
                            &crate::icons::CHECK_SQUARE
                        } else {
                            &crate::icons::SQUARE
                        })
                        .size(px(font_size::MD))
                        .color(low)
                        .into_any_element(),
                        (None, Some(first)) => format!("{}.", first + n as u64).into_any_element(),
                        (None, None) => "•".into_any_element(),
                    };
                    let marker_id = format!("{id}-{n}-marker");
                    div()
                        .flex()
                        .gap(px(spacing::SM))
                        .child(
                            div()
                                .flex_none()
                                .text_color(low)
                                .debug_selector(|| marker_id)
                                .child(marker),
                        )
                        .child(
                            blocks(theme, &format!("{id}-{n}"), &item.blocks)
                                .flex_1()
                                .min_w_0()
                                .gap(px(spacing::XS)),
                        )
                }))
        }
        Block::Quote(list) => blocks(theme, id, list)
            .pl(px(spacing::MD))
            .border_l_2()
            .border_color(rgba(theme.neutral_border))
            .text_color(low),
        Block::Table(rows) => div()
            .flex()
            .flex_col()
            .rounded(px(radius::MD))
            .border_1()
            .border_color(rgba(theme.neutral_border_subtle))
            .children(rows.iter().enumerate().map(|(r, row)| {
                div()
                    .flex()
                    .when(r > 0, |d| {
                        d.border_t_1()
                            .border_color(rgba(theme.neutral_border_subtle))
                    })
                    .when(r == 0, |d| d.font_weight(FontWeight::BOLD))
                    .children(row.iter().enumerate().map(|(c, cell)| {
                        div().flex_1().min_w_0().p(px(spacing::SM)).child(inline(
                            theme,
                            &format!("{id}-{r}-{c}"),
                            cell,
                        ))
                    }))
            })),
        Block::Rule => div().h(px(1.0)).bg(rgba(theme.neutral_border_subtle)),
    }
}

fn inline(theme: &Theme, id: &str, i: &Inline) -> AnyElement {
    let link = rgba(theme.accent_text_low);
    let code = rgba(theme.control);
    let highlights = i.marks.iter().map(|(range, m)| {
        let style = HighlightStyle {
            font_weight: m.bold.then_some(FontWeight::BOLD),
            font_style: m.italic.then_some(FontStyle::Italic),
            strikethrough: m.strike.then_some(StrikethroughStyle {
                thickness: px(1.0),
                color: None,
            }),
            color: m.link.then(|| link.into()),
            underline: m.link.then_some(UnderlineStyle {
                thickness: px(1.0),
                color: None,
                wavy: false,
            }),
            background_color: m.code.then(|| code.into()),
            ..Default::default()
        };
        (range.clone(), style)
    });
    let mono = i
        .marks
        .iter()
        .filter(|(_, m)| m.code)
        .map(|(range, _)| (range.clone(), SharedString::from(font::MONO_FAMILY)));
    let text = StyledText::new(i.text.clone())
        .with_highlights(highlights)
        .with_font_family_overrides(mono);
    if i.links.is_empty() {
        return text.into_any_element();
    }
    let urls: Vec<String> = i.links.iter().map(|(_, url)| url.clone()).collect();
    let ranges = i.links.iter().map(|(range, _)| range.clone()).collect();
    InteractiveText::new(ElementId::Name(SharedString::from(id.to_owned())), text)
        .on_click(ranges, move |n, _, cx| cx.open_url(&urls[n]))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(b: &Block) -> &Inline {
        match b {
            Block::Paragraph(i) | Block::Heading(_, i) => i,
            other => panic!("not text: {other:?}"),
        }
    }

    /// GitHub が作るリリースノートの形: 見出し・緩くない一覧・裸の URL・@ の名前。
    #[test]
    fn github_release_notes() {
        let notes = "## What's Changed\n* feat: logger by @someone in https://github.com/o/r/pull/4\n* fix: **bold** and `code`\n\n**Full Changelog**: https://github.com/o/r/commits/v1.";
        let b = parse(notes);
        assert_eq!(b.len(), 3, "{b:?}");
        assert_eq!(para(&b[0]).text, "What's Changed");
        assert!(matches!(b[0], Block::Heading(2, _)));
        let Block::List(None, items) = &b[1] else {
            panic!("{:?}", b[1])
        };
        let first = para(&items[0].blocks[0]);
        assert_eq!(
            first.links,
            [(
                first.text.find("https").unwrap()..first.text.len(),
                "https://github.com/o/r/pull/4".to_owned()
            )]
        );
        let second = para(&items[1].blocks[0]);
        assert_eq!(second.text, "fix: bold and code");
        let marks: Vec<_> = second
            .marks
            .iter()
            .map(|(r, m)| (&second.text[r.clone()], *m))
            .collect();
        assert_eq!(
            marks,
            [
                (
                    "bold",
                    Marks {
                        bold: true,
                        ..Default::default()
                    }
                ),
                (
                    "code",
                    Marks {
                        code: true,
                        ..Default::default()
                    }
                ),
            ]
        );
        let last = para(&b[2]);
        assert_eq!(
            last.links
                .iter()
                .map(|(_, u)| u.as_str())
                .collect::<Vec<_>>(),
            ["https://github.com/o/r/commits/v1"],
            "文末の句点は URL に含めない"
        );
    }

    #[test]
    fn links_lists_quotes_code_tables_and_rules() {
        let b = parse(
            "See [the docs](https://example.com/d) <!-- hidden -->\n\n3. three\n4. four\n\n- [x] done\n- [ ] todo\n\n> quoted\n\n```rust\nlet x = 1;\n```\n\n| a | b |\n|---|---|\n| 1 | ~~2~~ |\n\n---\n",
        );
        let doc = para(&b[0]);
        assert_eq!(doc.text.trim_end(), "See the docs");
        assert_eq!(doc.links, [(4..12, "https://example.com/d".to_owned())]);
        let Block::List(Some(3), items) = &b[1] else {
            panic!("{:?}", b[1])
        };
        assert_eq!(items.len(), 2);
        let Block::List(None, tasks) = &b[2] else {
            panic!("{:?}", b[2])
        };
        assert_eq!(
            tasks.iter().map(|i| i.checked).collect::<Vec<_>>(),
            [Some(true), Some(false)]
        );
        assert!(matches!(&b[3], Block::Quote(q) if para(&q[0]).text == "quoted"));
        assert_eq!(b[4], Block::Code("let x = 1;".into()));
        let Block::Table(rows) = &b[5] else {
            panic!("{:?}", b[5])
        };
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[1][1].marks[0].1,
            Marks {
                strike: true,
                ..Default::default()
            }
        );
        assert_eq!(b[6], Block::Rule);
        assert_eq!(b.len(), 7);
    }

    #[test]
    fn empty_and_html_only_notes_render_nothing() {
        assert!(parse("").is_empty());
        assert!(parse("<!-- nothing -->\n").is_empty());
    }
}
