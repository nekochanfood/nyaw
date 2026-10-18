//! 画面の語彙。ニャプリは「何を出すか」だけをこの木で宣言し、見た目（色・寸法・ホバー・押下の動き）はホストが
//! `design` の部品で決める。ホストとの間は JSON（`serde_json`）で受け渡す。
//!
//! 語彙は足すだけにして、消したり意味を変えたりしない（入っているニャプリが壊れる）。

use serde::{Deserialize, Serialize};

/// 画面の木の 1 つの要素。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Node {
    /// 子を縦に並べる。
    Column(Stack),
    /// 子を横に並べる。
    Row(Stack),
    /// カード（地の上に置く面）。中は縦に並ぶ。
    Card(Stack),
    /// 子を `columns` 列の格子に並べる。
    Grid {
        columns: u16,
        #[serde(default)]
        gap: Gap,
        children: Vec<Node>,
    },
    Text(Text),
    /// 押すと [`crate::Event::Click`]。
    Button(Button),
    /// 押すと [`crate::Event::Toggle`]（`on` は押した後の値）。
    Switch {
        id: String,
        on: bool,
    },
    /// 図形を並べて描く面。指すと [`crate::Event::Pointer`]（面の左上を原点とする px）。
    Canvas(Canvas),
    /// 並びの残りを埋める空き。
    Spacer,
}

/// 縦・横に並べる入れ物。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Stack {
    pub children: Vec<Node>,
    #[serde(default)]
    pub gap: Gap,
    /// 並びと直交する向きの揃え。
    #[serde(default)]
    pub align: Align,
    /// 親の並びの残りを埋める。
    #[serde(default)]
    pub grow: bool,
    /// はみ出したら縦にスクロールする。
    #[serde(default)]
    pub scroll: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Text {
    pub text: String,
    #[serde(default)]
    pub size: Size,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub color: Color,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Button {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub variant: Variant,
    /// 選んでいる切り替えボタン（押した色のままにする）。
    #[serde(default)]
    pub selected: bool,
    #[serde(default)]
    pub grow: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Canvas {
    pub id: String,
    pub width: f32,
    pub height: f32,
    pub shapes: Vec<Shape>,
}

/// 面に描く図形（px。面の左上が原点）。後のものほど手前。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum Shape {
    /// 中心 (`x`, `y`)・半径 `r` の円。
    Circle {
        x: f32,
        y: f32,
        r: f32,
        color: Color,
    },
    /// 左上 (`x`, `y`)・大きさ `w` × `h`、角丸 `radius` の四角。
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        #[serde(default)]
        radius: f32,
        color: Color,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gap {
    None,
    Xs,
    #[default]
    Sm,
    Md,
    Lg,
    Xl,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Stretch,
    Start,
    Center,
    End,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Size {
    Sm,
    #[default]
    Md,
    Lg,
    Xl,
    /// 電卓の表示・時計の数字のような、大きく見せる数字。
    Display,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Variant {
    #[default]
    Default,
    Primary,
    Danger,
    /// 背景の無いボタン。
    Ghost,
}

/// 色。役割で指すとテーマ（ダーク・ライト）に合わせてホストが選ぶ。`Rgb` だけは 0xRRGGBB をそのまま使う。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Color {
    #[default]
    Text,
    /// 控えめな文字。
    Low,
    Accent,
    Danger,
    /// アクセントの上に載せる文字。
    OnAccent,
    Card,
    Background,
    Rgb(u32),
}

pub fn column(children: impl IntoIterator<Item = Node>) -> Node {
    Node::Column(stack(children))
}

pub fn row(children: impl IntoIterator<Item = Node>) -> Node {
    Node::Row(stack(children))
}

pub fn card(children: impl IntoIterator<Item = Node>) -> Node {
    Node::Card(stack(children))
}

pub fn grid(columns: u16, children: impl IntoIterator<Item = Node>) -> Node {
    Node::Grid {
        columns,
        gap: Gap::default(),
        children: children.into_iter().collect(),
    }
}

pub fn text(text: impl Into<String>) -> Node {
    Node::Text(Text {
        text: text.into(),
        ..Default::default()
    })
}

pub fn button(id: impl Into<String>, label: impl Into<String>) -> Node {
    Node::Button(Button {
        id: id.into(),
        label: label.into(),
        ..Default::default()
    })
}

pub fn switch(id: impl Into<String>, on: bool) -> Node {
    Node::Switch { id: id.into(), on }
}

pub fn canvas(id: impl Into<String>, width: f32, height: f32, shapes: Vec<Shape>) -> Node {
    Node::Canvas(Canvas {
        id: id.into(),
        width,
        height,
        shapes,
    })
}

pub fn spacer() -> Node {
    Node::Spacer
}

fn stack(children: impl IntoIterator<Item = Node>) -> Stack {
    Stack {
        children: children.into_iter().collect(),
        ..Default::default()
    }
}

/// 組み立て用の書き換え。その要素に無い指定は無視する。
impl Node {
    fn stack_mut(&mut self) -> Option<&mut Stack> {
        match self {
            Node::Column(s) | Node::Row(s) | Node::Card(s) => Some(s),
            _ => None,
        }
    }

    pub fn gap(mut self, gap: Gap) -> Self {
        match &mut self {
            Node::Grid { gap: g, .. } => *g = gap,
            n => {
                if let Some(s) = n.stack_mut() {
                    s.gap = gap;
                }
            }
        }
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        if let Some(s) = self.stack_mut() {
            s.align = align;
        }
        self
    }

    pub fn grow(mut self) -> Self {
        match &mut self {
            Node::Button(b) => b.grow = true,
            n => {
                if let Some(s) = n.stack_mut() {
                    s.grow = true;
                }
            }
        }
        self
    }

    pub fn scroll(mut self) -> Self {
        if let Some(s) = self.stack_mut() {
            s.scroll = true;
        }
        self
    }

    pub fn size(mut self, size: Size) -> Self {
        if let Node::Text(t) = &mut self {
            t.size = size;
        }
        self
    }

    pub fn bold(mut self) -> Self {
        if let Node::Text(t) = &mut self {
            t.bold = true;
        }
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        if let Node::Text(t) = &mut self {
            t.color = color;
        }
        self
    }

    pub fn variant(mut self, variant: Variant) -> Self {
        if let Node::Button(b) = &mut self {
            b.variant = variant;
        }
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        if let Node::Button(b) = &mut self {
            b.selected = selected;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 木は JSON を行き来しても変わらない（ホストとの受け渡し）。省いた指定は既定値で読める。
    #[test]
    fn tree_round_trips_through_json() {
        let tree = column([
            text("0").size(Size::Display).bold(),
            row([
                button("a", "A").variant(Variant::Primary).grow(),
                switch("s", true),
            ])
            .gap(Gap::Lg),
            canvas(
                "c",
                10.0,
                10.0,
                vec![Shape::Circle {
                    x: 1.0,
                    y: 2.0,
                    r: 3.0,
                    color: Color::Rgb(0xff0000),
                }],
            ),
            spacer(),
        ]);
        let json = serde_json::to_string(&tree).unwrap();
        assert_eq!(serde_json::from_str::<Node>(&json).unwrap(), tree);
        let short: Node = serde_json::from_str(r#"{"type":"text","text":"hi"}"#).unwrap();
        assert_eq!(short, text("hi"));
    }
}
