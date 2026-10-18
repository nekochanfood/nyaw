//! 画面・ウィンドウの取得元の一覧を出す（名前・種類・実行ファイル名・アイコンの有無）。`cargo run -p desktop --example sources`

fn main() {
    let pending = desktop::Sources::refresh().expect("start listing");
    let sources = loop {
        if let Some(result) = pending.poll() {
            break result.expect("list sources");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    for s in sources {
        println!(
            "{:?}\t{}\tapp={:?}\ticon={}",
            s.kind,
            s.name,
            s.app,
            s.icon
                .as_ref()
                .map_or("none".into(), |i| format!("{}x{}", i.width, i.height)),
        );
    }
}
