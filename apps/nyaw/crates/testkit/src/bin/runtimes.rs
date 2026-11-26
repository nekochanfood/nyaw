//! A wizard that installs real runtimes for E2E (the equivalent of `npx playwright install`).
//!
//! ```text
//! cargo runtimes                       interactive: shows the status and asks whether to install what is missing
//! cargo runtimes list                  show the status only
//! cargo runtimes install [name..]      install (all if omitted). --dry-run only shows what would be done
//! cargo runtimes uninstall [name..]    remove what testkit installed
//! ```
//!
//! Names are `monado` / `steamvr`. `cargo runtimes` is an alias in `.cargo/config.toml`.

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
            Kind::parse(a).ok_or_else(|| format!("unknown runtime: {a} (monado / steamvr)"))?,
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
            "unknown command: {sub} (list / install / uninstall)"
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
                    "\nEverything is installed. Run with `cargo test -p testkit --test runtimes -- --ignored`"
                );
            }
            for k in missing {
                if ask(&format!("\nInstall {k}? ({}) [y/N] ", k.install_hint())) {
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
    println!("Install location: {}", runtimes::root().display());
    for k in Kind::ALL {
        match k.locate() {
            Some(p) => println!("  {:<8} installed     {}", k.name(), p.display()),
            None => println!("  {:<8} not installed {}", k.name(), k.install_hint()),
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
