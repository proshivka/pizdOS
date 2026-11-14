mod app;
mod sys;
mod term;

use std::env;
use std::io::{self, Read};
use std::process;
use std::thread;
use std::time::Duration;

use app::{App, Tab};
use term::{Key, RawSession, Screen, parse_key, terminal_size};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_help() {
    println!(
        "pizsoft-ui {VERSION} — zero-dependency TUI desktop for pizdOS 2.0\n\
         \n\
         USAGE:\n\
           pizsoft [OPTIONS]\n\
         \n\
         OPTIONS:\n\
           --ascii         Use ASCII box borders (+-+|)\n\
           --dump [1..4]   Render one 80x25 frame for tab 1..4 to stdout and exit\n\
           -h, --help      Show this help\n\
           -V, --version   Show version\n\
         \n\
         HOTKEYS:\n\
           1..4 / F1..F4 / Tab  Switch tabs (Sistema, Fayly, piz-get, Matrica)\n\
           Up / Down / Enter    Navigate lists\n\
           b                    Toggle Unicode / ASCII borders\n\
           q / F10              Quit"
    );
}

fn run_interactive(ascii_borders: bool) -> io::Result<()> {
    let _session = RawSession::enter()?;
    let (w, h) = terminal_size();
    let mut screen = Screen::new(w, h);
    let mut app = App::new(ascii_borders);
    let mut stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut buf = [0u8; 16];

    while app.running {
        let (nw, nh) = terminal_size();
        screen.resize(nw, nh);
        app.render(&mut screen);
        screen.flush(&mut stdout)?;

        match stdin.read(&mut buf) {
            Ok(0) => {
                thread::sleep(Duration::from_millis(40));
            }
            Ok(n) => {
                let key = parse_key(&buf[..n]);
                if key != Key::None {
                    app.handle_key(key);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }

        app.on_tick(screen.width, screen.height);
    }

    Ok(())
}

fn main() {
    let mut ascii_borders = false;
    let mut dump_tab: Option<Tab> = None;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return;
            }
            "-V" | "--version" => {
                println!("pizsoft-ui {VERSION}");
                return;
            }
            "--ascii" => ascii_borders = true,
            "--dump" => {
                let tab = match args.next().as_deref() {
                    Some("2") => Tab::Files,
                    Some("3") => Tab::Packages,
                    Some("4") => Tab::Matrix,
                    _ => Tab::Dashboard,
                };
                dump_tab = Some(tab);
            }
            other => {
                eprintln!("Unknown option: {other}");
                process::exit(1);
            }
        }
    }

    if let Some(tab) = dump_tab {
        let mut screen = Screen::new(80, 25);
        let mut app = App::new(ascii_borders);
        app.tab = tab;
        app.render(&mut screen);
        println!("{}", screen.render_plain());
        return;
    }

    if let Err(err) = run_interactive(ascii_borders) {
        eprintln!("pizsoft-ui error: {err}");
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::{MemInfo, parse_pkg_blocks};

    #[test]
    fn test_key_parser() {
        assert_eq!(parse_key(b"\x1b[A"), Key::Up);
        assert_eq!(parse_key(b"\x1b[B"), Key::Down);
        assert_eq!(parse_key(b"\x1b[C"), Key::Right);
        assert_eq!(parse_key(b"\x1b[D"), Key::Left);
        assert_eq!(parse_key(b"\x1bOP"), Key::F(1));
        assert_eq!(parse_key(b"\x1b[21~"), Key::F(10));
        assert_eq!(parse_key(b"\t"), Key::Tab);
        assert_eq!(parse_key(b"\r"), Key::Enter);
        assert_eq!(parse_key(b"\x7f"), Key::Backspace);
        assert_eq!(parse_key(b"q"), Key::Char('q'));
    }

    #[test]
    fn test_meminfo_and_pkg_db_parsing() {
        let mem_raw = "MemTotal:        1048576 kB\nMemFree:          262144 kB\nMemAvailable:     524288 kB\n";
        let mem = MemInfo::parse_proc_meminfo(mem_raw);
        assert_eq!(mem.total_mb, 1024);
        assert_eq!(mem.used_mb, 512);
        assert_eq!(mem.percent, 50);

        let db_raw = "name=pizfetch\nversion=0.1.0\narch=x86_64\ndesc=System info\n---\nname=pizsoft-ui\nversion=0.1.0\narch=x86_64\ndesc=Desktop\n";
        let pkgs = parse_pkg_blocks(db_raw, true);
        assert_eq!(pkgs.len(), 2);
        assert_eq!(pkgs[0].name, "pizfetch");
        assert!(pkgs[0].installed);
        assert_eq!(pkgs[1].name, "pizsoft-ui");
    }

    #[test]
    fn test_render_all_tabs() {
        let mut screen = Screen::new(80, 25);
        let mut app = App::new(false);

        for (tab, expected) in [
            (Tab::Dashboard, "Sistema"),
            (Tab::Files, "Fayly"),
            (Tab::Packages, "piz-get"),
            (Tab::Matrix, "Matrica"),
        ] {
            app.tab = tab;
            app.render(&mut screen);
            let out = screen.render_plain();
            assert!(out.contains("PIZSOFT UI"));
            assert!(out.contains(expected), "missing {expected} in {tab:?}");
        }
    }
}
