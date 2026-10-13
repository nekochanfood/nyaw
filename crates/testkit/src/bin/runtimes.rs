//! E2E 用の実ランタイムを入れるウィザード（`npx playwright install` に当たる）。
//!
//! ```text
//! cargo runtimes                       対話式: 状態を見せて、入っていないものを入れるか聞く
//! cargo runtimes list                  状態だけ見る
//! cargo runtimes install [名前..]      入れる（省略で全部）。--dry-run で何をするかだけ表示
//! cargo runtimes uninstall [名前..]    testkit が入れたものを消す
//! ```
//!
//! 名前は `monado` / `steamvr`。`cargo runtimes` は `.cargo/config.toml` の別名。

use std::io::{BufRead, Write};
use std::process::ExitCode;

use testkit::runtimes::{self, Kind};

#[derive(Debug, PartialEq)]
enum Cmd {
    Wizard,
    List,
    Install { kinds: Vec<Kind>, dry_run: bool },
    Uninstall(Vec<Kind>),
}

fn parse(args: &[String]) -> Result<Cmd, String> {
    let (sub, rest) = match args.split_first() {
        None => return Ok(Cmd::Wizard),
        Some((sub, rest)) => (sub.as_str(), rest),
    };
    let dry_run = rest.iter().any(|a| a == "--dry-run");
    let mut kinds = vec![];
    for a in rest.iter().filter(|a| *a != "--dry-run") {
        kinds.push(
            Kind::parse(a).ok_or_else(|| format!("知らないランタイム: {a}（monado / steamvr）"))?,
        );
    }
    if kinds.is_empty() {
        kinds = Kind::ALL.to_vec();
    }
    match sub {
        "list" => Ok(Cmd::List),
        "install" => Ok(Cmd::Install { kinds, dry_run }),
        "uninstall" => Ok(Cmd::Uninstall(kinds)),
        _ => Err(format!(
            "知らないコマンド: {sub}（list / install / uninstall）"
        )),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = match parse(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let mut ok = true;
    match cmd {
        Cmd::List => list(),
        Cmd::Install { kinds, dry_run } => {
            for k in kinds {
                ok &= report(runtimes::install(k, dry_run));
            }
        }
        Cmd::Uninstall(kinds) => {
            for k in kinds {
                ok &= report(runtimes::uninstall(k));
            }
        }
        Cmd::Wizard => {
            list();
            let missing: Vec<Kind> = Kind::ALL
                .into_iter()
                .filter(|k| k.locate().is_none())
                .collect();
            if missing.is_empty() {
                println!(
                    "\n全部入っている。`cargo test -p testkit --test runtimes -- --ignored` で流せる"
                );
            }
            for k in missing {
                if ask(&format!("\n{k} を入れる？ ({}) [y/N] ", k.install_hint())) {
                    ok &= report(runtimes::install(k, false));
                }
            }
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn list() {
    println!("インストール先: {}", runtimes::root().display());
    for k in Kind::ALL {
        match k.locate() {
            Some(p) => println!("  {:<8} 入っている   {}", k.name(), p.display()),
            None => println!("  {:<8} 入っていない {}", k.name(), k.install_hint()),
        }
    }
}

fn report(r: Result<(), String>) -> bool {
    r.map_err(|e| eprintln!("{e}")).is_ok()
}

fn ask(question: &str) -> bool {
    print!("{question}");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    matches!(line.trim(), "y" | "Y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn parses_like_playwright_install() {
        assert_eq!(parse(&args("")), Ok(Cmd::Wizard));
        assert_eq!(parse(&args("list")), Ok(Cmd::List));
        assert_eq!(
            parse(&args("install")),
            Ok(Cmd::Install {
                kinds: Kind::ALL.to_vec(),
                dry_run: false
            })
        );
        assert_eq!(
            parse(&args("install monado --dry-run")),
            Ok(Cmd::Install {
                kinds: vec![Kind::Monado],
                dry_run: true
            })
        );
        assert_eq!(
            parse(&args("uninstall steamvr")),
            Ok(Cmd::Uninstall(vec![Kind::SteamVr]))
        );
        assert!(parse(&args("install oculus")).is_err());
        assert!(parse(&args("frobnicate")).is_err());
    }
}
