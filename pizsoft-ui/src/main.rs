mod apps;
mod desktop;
mod font;
mod gfx;
mod platform;
mod sys;

use std::env;
use std::path::PathBuf;
use std::process;

use desktop::Desktop;
use gfx::Canvas;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_help() {
    println!(
        "pizsoft-ui {VERSION} — zero-dependency pixel GUI for pizdOS 2.0\n\
         \n\
         USAGE:\n\
           pizsoft [OPTIONS]\n\
         \n\
         OPTIONS:\n\
           --dump-bmp <FILE>   Render 960x600 desktop screenshot to BMP and exit\n\
           -h, --help          Show this help\n\
           -V, --version       Show version"
    );
}

fn main() {
    let mut dump_bmp: Option<PathBuf> = None;
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
            "--dump-bmp" => {
                let Some(path) = args.next() else {
                    eprintln!("--dump-bmp requires a file path");
                    process::exit(1);
                };
                dump_bmp = Some(PathBuf::from(path));
            }
            other => {
                eprintln!("Unknown option: {other}");
                process::exit(1);
            }
        }
    }

    if let Some(path) = dump_bmp {
        let mut canvas = Canvas::new(960, 600);
        let desktop = Desktop::new(960, 600);
        desktop.render(&mut canvas);
        if let Err(e) = canvas.write_bmp(&path) {
            eprintln!("Failed to write {}: {e}", path.display());
            process::exit(1);
        }
        println!("Saved {} (960x600 BMP)", path.display());
        return;
    }

    if let Err(err) = platform::run_gui() {
        eprintln!("pizsoft-ui error: {err}");
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::{MemInfo, parse_pkg_blocks};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn test_meminfo_and_pkg_db_parsing() {
        let mem_raw =
            "MemTotal:        1048576 kB\nMemFree:          262144 kB\nMemAvailable:     524288 kB\n";
        let mem = MemInfo::parse_proc_meminfo(mem_raw);
        assert_eq!(mem.total_mb, 1024);
        assert_eq!(mem.used_mb, 512);
        assert_eq!(mem.percent, 50);

        let db_raw = "name=pizfetch\nversion=0.1.0\narch=x86_64\ndesc=System info\n---\nname=pizsoft-ui\nversion=0.1.0\narch=x86_64\ndesc=Desktop\n";
        let pkgs = parse_pkg_blocks(db_raw, true);
        assert_eq!(pkgs.len(), 2);
        assert_eq!(pkgs[0].name, "pizfetch");
        assert!(pkgs[0].installed);
    }

    #[test]
    fn test_desktop_render_and_bmp() {
        let mut canvas = Canvas::new(960, 600);
        let mut desktop = Desktop::new(960, 600);

        // Drag topmost window ("Nastroyki" at 370,90)
        let orig_x = desktop.windows.last().unwrap().x;
        desktop.on_mouse_down(390, 100, 960, 600);
        desktop.on_mouse_move(440, 120, 960, 600);
        desktop.on_mouse_up();
        assert_ne!(desktop.windows.last().unwrap().x, orig_x);

        // Open Start Menu & all new apps + toggle RU/EN language
        desktop.on_mouse_down(20, 580, 960, 600);
        assert!(desktop.start_menu_open);
        desktop.on_mouse_down(820, 580, 960, 600); // click [RU/EN] toggle on taskbar
        assert_eq!(desktop.settings.lang, apps::Lang::En);
        desktop.focus_or_open(desktop::AppId::Terminal);
        desktop.focus_or_open(desktop::AppId::Paint);
        desktop.focus_or_open(desktop::AppId::Snake);
        desktop.focus_or_open(desktop::AppId::Notepad);

        desktop.render(&mut canvas);
        assert!(canvas.pixels.iter().any(|&p| p != 0x101822));

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let bmp_path = env::temp_dir().join(format!("pizsoft_gui_{nanos}.bmp"));
        canvas.write_bmp(&bmp_path).unwrap();
        let bytes = fs::read(&bmp_path).unwrap();
        assert!(bytes.starts_with(b"BM"));
        let _ = fs::remove_file(&bmp_path);
    }
}
