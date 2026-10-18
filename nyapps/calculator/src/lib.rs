//! 電卓（net.bettaworx.calculator）。押した順に計算する、ふつうの電卓。

use nyapp_sdk::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
}

impl Op {
    fn apply(self, a: f64, b: f64) -> f64 {
        match self {
            Op::Add => a + b,
            Op::Sub => a - b,
            Op::Mul => a * b,
            Op::Div => a / b,
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            Op::Add => "+",
            Op::Sub => "−",
            Op::Mul => "×",
            Op::Div => "÷",
        }
    }
}

/// 表示できる桁の上限。
const MAX_DIGITS: usize = 15;

#[derive(Debug)]
pub struct Calculator {
    /// 表示している数（打っている途中の文字列）。
    entry: String,
    /// 演算子の左の数と演算子。
    pending: Option<(f64, Op)>,
    /// 次の数字で表示を打ち直す（演算子・= の直後）。
    fresh: bool,
}

impl Calculator {
    fn value(&self) -> f64 {
        self.entry.parse().unwrap_or(0.0)
    }

    /// キー `key`（`0`〜`9`・`.`・`+`・`-`・`*`・`/`・`=`・`c`・`neg`・`pct`・`back`）を押す。
    pub fn press(&mut self, key: &str) {
        let op = match key {
            "+" => Some(Op::Add),
            "-" => Some(Op::Sub),
            "*" => Some(Op::Mul),
            "/" => Some(Op::Div),
            _ => None,
        };
        if let Some(op) = op {
            let value = match self.pending {
                // 続けて演算子を押したら、そこまでを計算する（押し直しは演算子だけ替える）。
                Some((left, prev)) if !self.fresh => prev.apply(left, self.value()),
                Some((left, _)) => left,
                None => self.value(),
            };
            self.entry = format(value);
            self.pending = Some((value, op));
            self.fresh = true;
            return;
        }
        match key {
            "=" => {
                if let Some((left, op)) = self.pending.take() {
                    self.entry = format(op.apply(left, self.value()));
                }
                self.fresh = true;
            }
            "c" => *self = Self::new(),
            "neg" => {
                self.entry = match self.entry.strip_prefix('-') {
                    Some(rest) => rest.to_owned(),
                    None if self.entry != "0" => format!("-{}", self.entry),
                    None => self.entry.clone(),
                };
            }
            "pct" => {
                self.entry = format(self.value() / 100.0);
                self.fresh = true;
            }
            "back" if !self.fresh => {
                self.entry.pop();
                if self.entry.is_empty() || self.entry == "-" {
                    self.entry = "0".into();
                }
            }
            "." => {
                if self.fresh || self.entry == "Error" {
                    self.entry = "0".into();
                    self.fresh = false;
                }
                if !self.entry.contains('.') {
                    self.entry.push('.');
                }
            }
            d if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() => {
                if self.fresh || self.entry == "Error" {
                    self.entry = "0".into();
                    self.fresh = false;
                }
                if self.entry == "0" {
                    self.entry = d.into();
                } else if self.entry.chars().filter(char::is_ascii_digit).count() < MAX_DIGITS {
                    self.entry.push_str(d);
                }
            }
            _ => {}
        }
    }

    pub fn display(&self) -> &str {
        &self.entry
    }
}

/// 計算結果を表示の文字列にする（整数は小数点なし、そうでなければ末尾の 0 を落とす）。
fn format(v: f64) -> String {
    if !v.is_finite() {
        return "Error".into();
    }
    if v.fract() == 0.0 && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let s = format!("{v:.10}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// 並べるキー: (ID, 表示, 見た目)。
const KEYS: [(&str, &str, Variant); 20] = [
    ("c", "C", Variant::Danger),
    ("back", "⌫", Variant::Default),
    ("pct", "%", Variant::Default),
    ("/", "÷", Variant::Default),
    ("7", "7", Variant::Ghost),
    ("8", "8", Variant::Ghost),
    ("9", "9", Variant::Ghost),
    ("*", "×", Variant::Default),
    ("4", "4", Variant::Ghost),
    ("5", "5", Variant::Ghost),
    ("6", "6", Variant::Ghost),
    ("-", "−", Variant::Default),
    ("1", "1", Variant::Ghost),
    ("2", "2", Variant::Ghost),
    ("3", "3", Variant::Ghost),
    ("+", "+", Variant::Default),
    ("neg", "±", Variant::Ghost),
    ("0", "0", Variant::Ghost),
    (".", ".", Variant::Ghost),
    ("=", "=", Variant::Primary),
];

impl App for Calculator {
    fn new() -> Self {
        Self {
            entry: "0".into(),
            pending: None,
            fresh: false,
        }
    }

    fn view(&mut self) -> Node {
        let expression = match self.pending {
            Some((left, op)) => format!("{} {}", format(left), op.symbol()),
            None => String::new(),
        };
        let current_op = self
            .pending
            .filter(|_| self.fresh)
            .map(|(_, op)| op.symbol());
        let keys = KEYS.iter().map(|&(id, label, variant)| {
            button(format!("key-{id}"), label)
                .variant(variant)
                .selected(Some(label) == current_op)
        });
        column([
            card([
                text(expression).size(Size::Lg).color(Color::Low),
                text(self.entry.clone()).size(Size::Display).bold(),
            ])
            .align(Align::End),
            grid(4, keys).gap(Gap::Sm),
        ])
        .gap(Gap::Md)
        .grow()
    }

    fn event(&mut self, event: Event) {
        if let Event::Click { id } = event
            && let Some(key) = id.strip_prefix("key-")
        {
            self.press(key);
        }
    }
}

export!(Calculator);

#[cfg(test)]
mod tests {
    use super::*;

    fn run(keys: &str) -> String {
        let mut c = Calculator::new();
        for key in keys.split_whitespace() {
            c.press(key);
        }
        c.display().to_owned()
    }

    #[test]
    fn calculates_in_the_order_pressed() {
        assert_eq!(run("1 2 + 3 ="), "15");
        assert_eq!(run("2 + 3 * 4 ="), "20", "押した順（優先順位は付けない）");
        assert_eq!(run("1 0 / 4 ="), "2.5");
        assert_eq!(run("1 / 0 ="), "Error");
        assert_eq!(run("1 / 0 = 5"), "5", "エラーの後は打ち直せる");
        assert_eq!(run("5 + * 2 ="), "10", "演算子の押し直しは演算子だけ替える");
        assert_eq!(run("0 . 1 + 0 . 2 ="), "0.3");
    }

    #[test]
    fn editing_keys() {
        assert_eq!(run("1 2 3 back"), "12");
        assert_eq!(run("5 neg"), "-5");
        assert_eq!(run("5 0 pct"), "0.5");
        assert_eq!(run("1 . . 5"), "1.5");
        assert_eq!(run("7 c"), "0");
        assert_eq!(run(&"9 ".repeat(20)).len(), MAX_DIGITS);
    }
}
