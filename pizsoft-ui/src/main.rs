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
        desktop.focus_or_open(desktop::AppId::Calc);
        desktop.focus_or_open(desktop::AppId::Matrix);

        desktop.calc.press("2");
        desktop.calc.press("+");
        desktop.calc.press("3");
        desktop.calc.press("=");
        assert_eq!(desktop.calc.display, "5");

        desktop.on_right_click(500, 300, 960, 600);
        assert!(desktop.ctx_menu.is_some());

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

    #[test]
    fn test_window_resize_and_interactive_about_os() {
        let mut desktop = Desktop::new(1160, 720);

        // 1. Resize topmost window ("Settings" at 370, 90, 520, 344) via bottom-right corner
        let sx = desktop.windows.last().unwrap().x + desktop.windows.last().unwrap().w - 3;
        let sy = desktop.windows.last().unwrap().y + desktop.windows.last().unwrap().h - 3;
        desktop.on_mouse_down(sx, sy, 1160, 720);
        assert!(desktop.resizing.is_some());
        desktop.on_mouse_move(sx + 60, sy + 50, 1160, 720);
        desktop.on_mouse_up();
        assert_eq!(desktop.windows.last().unwrap().w, 580);
        assert_eq!(desktop.windows.last().unwrap().h, 394);

        // Shrink below minimum size -> clamps to AppId::Settings.min_size() (440, 310)
        let sx2 = desktop.windows.last().unwrap().x + desktop.windows.last().unwrap().w - 3;
        let sy2 = desktop.windows.last().unwrap().y + desktop.windows.last().unwrap().h - 3;
        desktop.on_mouse_down(sx2, sy2, 1160, 720);
        desktop.on_mouse_move(100, 100, 1160, 720);
        desktop.on_mouse_up();
        assert_eq!(desktop.windows.last().unwrap().w, 440);
        assert_eq!(desktop.windows.last().unwrap().h, 310);

        // 2. Click Hero Banner in About OS -> increments update taps
        let win = desktop.windows.last().unwrap().clone();
        desktop.on_mouse_down(win.x + 200, win.y + 70, 1160, 720);
        assert_eq!(desktop.settings.update_taps, 1);

        // 3. Click Device card in About OS -> inline device rename via keyboard + Enter
        desktop.on_mouse_down(win.x + 190, win.y + 165, 1160, 720);
        assert!(desktop.settings.editing_device);
        for _ in 0..10 {
            desktop.on_key(apps::KeyEvent::Backspace);
        }
        for c in "ProBox".chars() {
            desktop.on_key(apps::KeyEvent::Char(c));
        }
        desktop.on_key(apps::KeyEvent::Enter);
        assert!(!desktop.settings.editing_device);
        assert_eq!(desktop.settings.device_name, "ProBox");

        // 4. Click Packages row in bottom ColorOS list -> opens PizGet window
        desktop.on_mouse_down(win.x + 200, win.y + 272, 1160, 720);
        assert_eq!(desktop.windows.last().unwrap().id, desktop::AppId::PizGet);
        assert!(desktop.windows.last().unwrap().open);

        // 5. Click tray clock/uptime area -> toggles show_clock
        let orig_clock = desktop.settings.show_clock;
        desktop.on_mouse_down(1100, 700, 1160, 720);
        assert_ne!(desktop.settings.show_clock, orig_clock);
    }
}
