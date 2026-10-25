//! xkeyboard-config の pc104 / pc105 / jp106 に対応するキー名と物理位置（1u 単位）。
use super::Keyboard;

/// キーボードの種類。設定ファイルでは `full` / `tenkeyless` / `compact`。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyboardType {
    #[default]
    Full,
    Tenkeyless,
    Compact,
}

impl KeyboardType {
    pub const ALL: [Self; 3] = [Self::Full, Self::Tenkeyless, Self::Compact];
    pub fn width(self) -> f32 {
        match self {
            Self::Full => 23.,
            Self::Tenkeyless => 18.5,
            Self::Compact => 15.,
        }
    }
    pub fn height(self) -> f32 {
        if self == Self::Compact { 5. } else { 6.5 }
    }
}

#[derive(Clone, Debug)]
pub struct Keycap {
    pub name: String,
    pub code: u16,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub shape: KeyShape,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KeyShape {
    Rectangle,
    /// 上段が 1.5u、下段が 1.25u の ISO / JIS Enter。
    SteppedEnter,
}

impl Keycap {
    /// ギャップを輪郭の両側に半分ずつ取り、凹角でも隣のキーと同じ間隔にする。
    pub fn outline(&self, unit: f32, gap: f32) -> Vec<(f32, f32)> {
        let (w, h) = (self.width * unit - gap, self.height * unit - gap);
        match self.shape {
            KeyShape::Rectangle => vec![(0., 0.), (w, 0.), (w, h), (0., h)],
            KeyShape::SteppedEnter => vec![
                (0., 0.),
                (w, 0.),
                (w, h),
                (unit * 0.25, h),
                (unit * 0.25, unit - gap),
                (0., unit - gap),
            ],
        }
    }

    /// キーが占める領域。Enter の左下の空白をヒット領域に含めない。
    pub fn regions(&self) -> Vec<(f32, f32, f32, f32)> {
        match self.shape {
            KeyShape::Rectangle => vec![(self.x, self.y, self.width, self.height)],
            KeyShape::SteppedEnter => vec![
                (self.x, self.y, self.width, 1.),
                (
                    self.x + 0.25,
                    self.y + 1.,
                    self.width - 0.25,
                    self.height - 1.,
                ),
            ],
        }
    }
}

pub fn keycaps(keyboard: &Keyboard, kind: KeyboardType) -> Vec<Keycap> {
    let mut caps = Vec::new();
    let mut add = |name: &str, x, y, width, height| {
        if let Some(code) = keyboard.code(name) {
            caps.push(Keycap {
                name: name.into(),
                code,
                x,
                y,
                width,
                height,
                shape: if name == "RTRN" && height == 2. {
                    KeyShape::SteppedEnter
                } else {
                    KeyShape::Rectangle
                },
            });
        }
    };
    let y = if kind == KeyboardType::Compact {
        0.
    } else {
        1.5
    };
    let jis = keyboard.layout.name == "jp";
    let iso = !matches!(keyboard.layout.model(), "pc104" | "jp106" | "kr106");
    add("TLDE", 0., y, 1., 1.);
    for i in 1..=12 {
        add(&format!("AE{i:02}"), i as f32, y, 1., 1.);
    }
    if jis {
        add("AE13", 13., y, 1., 1.);
    }
    add(
        "BKSP",
        if jis { 14. } else { 13. },
        y,
        if jis { 1. } else { 2. },
        1.,
    );
    add("TAB", 0., y + 1., 1.5, 1.);
    for i in 1..=12 {
        add(&format!("AD{i:02}"), 0.5 + i as f32, y + 1., 1., 1.);
    }
    if iso || jis {
        add("RTRN", 13.5, y + 1., 1.5, 2.);
    } else {
        add("BKSL", 13.5, y + 1., 1.5, 1.);
    }
    add("CAPS", 0., y + 2., 1.75, 1.);
    for i in 1..=11 {
        add(&format!("AC{i:02}"), 0.75 + i as f32, y + 2., 1., 1.);
    }
    if iso || jis {
        add("BKSL", 12.75, y + 2., 1., 1.);
    } else {
        add("RTRN", 12.75, y + 2., 2.25, 1.);
    }
    add("LFSH", 0., y + 3., if iso { 1.25 } else { 2.25 }, 1.);
    if iso {
        add("LSGT", 1.25, y + 3., 1., 1.);
    }
    for i in 1..=10 {
        add(&format!("AB{i:02}"), 1.25 + i as f32, y + 3., 1., 1.);
    }
    if jis {
        add("AB11", 12.25, y + 3., 1., 1.);
    }
    add(
        "RTSH",
        if jis { 13.25 } else { 12.25 },
        y + 3.,
        if jis { 1.75 } else { 2.75 },
        1.,
    );
    for (name, x) in [("LCTL", 0.), ("LWIN", 1.25), ("LALT", 2.5)] {
        add(name, x, y + 4., 1.25, 1.);
    }
    if jis {
        add("MUHE", 3.75, y + 4., 1., 1.);
        add("SPCE", 4.75, y + 4., 3.5, 1.);
        add("HENK", 8.25, y + 4., 1., 1.);
        add("HKTG", 9.25, y + 4., 1., 1.);
        add("RALT", 10.25, y + 4., 1., 1.);
    } else {
        add("SPCE", 3.75, y + 4., 6.25, 1.);
        add("RALT", 10., y + 4., 1.25, 1.);
    }
    for (name, x) in [("RWIN", 11.25), ("MENU", 12.5), ("RCTL", 13.75)] {
        add(name, x, y + 4., 1.25, 1.);
    }
    if kind != KeyboardType::Compact {
        add("ESC", 0., 0., 1., 1.);
        for i in 1..=12 {
            add(
                &format!("FK{i:02}"),
                i as f32 + 1. + ((i - 1) / 4) as f32 * 0.5,
                0.,
                1.,
                1.,
            );
        }
        for (name, x, yy) in [
            ("PRSC", 15.5, 0.),
            ("SCLK", 16.5, 0.),
            ("PAUS", 17.5, 0.),
            ("INS", 15.5, 1.5),
            ("HOME", 16.5, 1.5),
            ("PGUP", 17.5, 1.5),
            ("DELE", 15.5, 2.5),
            ("END", 16.5, 2.5),
            ("PGDN", 17.5, 2.5),
            ("UP", 16.5, 4.5),
            ("LEFT", 15.5, 5.5),
            ("DOWN", 16.5, 5.5),
            ("RGHT", 17.5, 5.5),
        ] {
            add(name, x, yy, 1., 1.);
        }
    }
    if kind == KeyboardType::Full {
        for (name, x, yy, w, h) in [
            ("NMLK", 19., 1.5, 1., 1.),
            ("KPDV", 20., 1.5, 1., 1.),
            ("KPMU", 21., 1.5, 1., 1.),
            ("KPSU", 22., 1.5, 1., 1.),
            ("KP7", 19., 2.5, 1., 1.),
            ("KP8", 20., 2.5, 1., 1.),
            ("KP9", 21., 2.5, 1., 1.),
            ("KPAD", 22., 2.5, 1., 2.),
            ("KP4", 19., 3.5, 1., 1.),
            ("KP5", 20., 3.5, 1., 1.),
            ("KP6", 21., 3.5, 1., 1.),
            ("KP1", 19., 4.5, 1., 1.),
            ("KP2", 20., 4.5, 1., 1.),
            ("KP3", 21., 4.5, 1., 1.),
            ("KPEN", 22., 4.5, 1., 2.),
            ("KP0", 19., 5.5, 2., 1.),
            ("KPDL", 21., 5.5, 1., 1.),
        ] {
            add(name, x, yy, w, h);
        }
    }
    caps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Database, Layout};
    #[test]
    fn types_have_distinct_keys_and_no_overlaps() {
        let db = Database::load().unwrap();
        for layout in ["us", "de", "jp"] {
            let keyboard = db.keyboard(&Layout::new(layout, "")).unwrap();
            for kind in KeyboardType::ALL {
                let keys = keyboard.keycaps(kind);
                assert_eq!(
                    keys.iter().any(|k| k.name == "KP7"),
                    kind == KeyboardType::Full
                );
                assert_eq!(
                    keys.iter().any(|k| k.name == "FK01"),
                    kind != KeyboardType::Compact
                );
                for (i, a) in keys.iter().enumerate() {
                    assert!(
                        a.x + a.width <= kind.width() && a.y + a.height <= kind.height(),
                        "{layout}: {}",
                        a.name
                    );
                    for b in &keys[i + 1..] {
                        for (x, y, w, h) in a.regions() {
                            for (xx, yy, ww, hh) in b.regions() {
                                assert!(
                                    x + w <= xx || xx + ww <= x || y + h <= yy || yy + hh <= y,
                                    "{layout}: {} overlaps {}",
                                    a.name,
                                    b.name
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn jis_enter_keeps_the_same_gap_at_the_top_and_the_step() {
        let db = Database::load().unwrap();
        let keyboard = db.keyboard(&crate::Layout::new("jp", "")).unwrap();
        let keys = keyboard.keycaps(KeyboardType::Compact);
        let enter = keys.iter().find(|k| k.name == "RTRN").unwrap();
        let bracket = keys.iter().find(|k| k.name == "AD12").unwrap();
        let backslash = keys.iter().find(|k| k.name == "BKSL").unwrap();
        assert_eq!(enter.shape, KeyShape::SteppedEnter);
        for gap in [0., 4., 8., 16.] {
            let outline = enter.outline(64., gap);
            assert_eq!(
                outline,
                vec![
                    (0., 0.),
                    (96. - gap, 0.),
                    (96. - gap, 128. - gap),
                    (16., 128. - gap),
                    (16., 64. - gap),
                    (0., 64. - gap)
                ]
            );
            let top_left = enter.x * 64. + gap / 2.;
            let bracket_right = (bracket.x + bracket.width) * 64. - gap / 2.;
            let lower_left = top_left + outline[3].0;
            let backslash_right = (backslash.x + backslash.width) * 64. - gap / 2.;
            assert_eq!(top_left - bracket_right, gap);
            assert_eq!(lower_left - backslash_right, gap);
        }
    }
}
