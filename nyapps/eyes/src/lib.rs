//! 目玉（net.bettaworx.eyes）。xeyes のように、2 つの目がポインタを追いかける。面（canvas）とポインタの出来事の確認用。

use nyapp_sdk::*;

const W: f32 = 600.0;
const H: f32 = 320.0;
const EYE_R: f32 = 120.0;
const PUPIL_R: f32 = 36.0;
/// 黒目が白目の縁から離れる幅。
const RIM: f32 = 10.0;
const EYES: [(f32, f32); 2] = [(W / 2.0 - 140.0, H / 2.0), (W / 2.0 + 140.0, H / 2.0)];

pub struct Eyes {
    /// 面の左上からのポインタの位置（まだ来ていなければ正面を見る）。
    pointer: Option<(f32, f32)>,
}

/// 中心 `eye` の目の黒目の位置: ポインタの方へ、白目からはみ出さない所まで寄せる。
pub fn pupil(eye: (f32, f32), pointer: Option<(f32, f32)>) -> (f32, f32) {
    let Some((px, py)) = pointer else {
        return eye;
    };
    let (dx, dy) = (px - eye.0, py - eye.1);
    let d = (dx * dx + dy * dy).sqrt();
    let reach = EYE_R - PUPIL_R - RIM;
    if d <= reach {
        return (px, py);
    }
    (eye.0 + dx / d * reach, eye.1 + dy / d * reach)
}

impl App for Eyes {
    fn new() -> Self {
        Self { pointer: None }
    }

    fn view(&mut self) -> Node {
        let mut shapes = Vec::new();
        for eye in EYES {
            let (x, y) = pupil(eye, self.pointer);
            shapes.extend([
                Shape::Circle {
                    x: eye.0,
                    y: eye.1,
                    r: EYE_R + 6.0,
                    color: Color::Text,
                },
                Shape::Circle {
                    x: eye.0,
                    y: eye.1,
                    r: EYE_R,
                    color: Color::Rgb(0xffffff),
                },
                Shape::Circle {
                    x,
                    y,
                    r: PUPIL_R,
                    color: Color::Rgb(0x111111),
                },
            ]);
        }
        let hint = if host::language() == "ja" {
            "パネルの上でポインタを動かしてみて"
        } else {
            "Move the pointer over the panel"
        };
        column([canvas("eyes", W, H, shapes), text(hint).color(Color::Low)])
            .align(Align::Center)
            .gap(Gap::Lg)
            .grow()
    }

    fn event(&mut self, event: Event) {
        if let Event::Pointer { id, x, y } = event
            && id == "eyes"
        {
            self.pointer = Some((x, y));
        }
    }
}

export!(Eyes);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pupils_follow_the_pointer_but_stay_inside_the_eye() {
        let eye = (100.0, 100.0);
        assert_eq!(pupil(eye, None), eye, "ポインタが来るまでは正面");
        assert_eq!(
            pupil(eye, Some((110.0, 100.0))),
            (110.0, 100.0),
            "近ければそこを見る"
        );
        let (x, y) = pupil(eye, Some((1000.0, 100.0)));
        assert!((x - (100.0 + EYE_R - PUPIL_R - RIM)).abs() < 1e-3 && y == 100.0);
    }
}
