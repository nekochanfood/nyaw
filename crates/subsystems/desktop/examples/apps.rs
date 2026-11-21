//! インストール済みアプリの一覧を出力する（名前・実行ファイル名・アイコンの有無）。所要時間も表示する。
//! `cargo run -p desktop --example apps`

fn main() {
    let start = std::time::Instant::now();
    let pending = desktop::InstalledApps::refresh().expect("start listing");
    let apps = loop {
        if let Some(result) = pending.poll() {
            break result.expect("list apps");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    for a in &apps {
        println!(
            "{}\texe={:?}\ticon={}",
            a.name,
            a.exe,
            a.icon
                .as_ref()
                .map_or("none".into(), |i| format!("{}x{}", i.width, i.height)),
        );
    }
    println!("{} apps in {:?}", apps.len(), start.elapsed());
}
