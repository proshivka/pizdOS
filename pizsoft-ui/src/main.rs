mod apps;
mod config;
mod desktop;
mod display_srv;
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
           --dump-bmp <FILE>   Render 800x600 desktop screenshot to BMP and exit\n\
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
        let mut canvas = Canvas::new(800, 600);
        let mut desktop = Desktop::new(800, 600);
        desktop.settings.ui_scale_pct = 100;
        canvas.set_ui_scale(1.0);
        desktop.on_viewport_resize(canvas.logical_width(), canvas.logical_height());
        desktop.render(&mut canvas);
        if let Err(e) = canvas.write_bmp(&path) {
            eprintln!("Failed to write {}: {e}", path.display());
            process::exit(1);
        }
        println!("Saved {} (800x600 BMP)", path.display());
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

    #[test]
    fn test_swar_blend_customization_and_animations() {
        // 1. SWAR blend_rgb accuracy across alpha 0..=255
        let samples = [(0x102030, 0xF43F5E), (0x000000, 0xFFFFFF), (0xAABBCC, 0x112233)];
        for (dst, src) in samples {
            assert_eq!(gfx::blend_rgb(dst, src, 0), dst & 0x00FF_FFFF);
            assert_eq!(gfx::blend_rgb(dst, src, 255), src & 0x00FF_FFFF);
            for a in 0..=255u8 {
                let got = gfx::blend_rgb(dst, src, a);
                let inv = 255 - a as u32;
                let au = a as u32;
                for shift in [16, 8, 0] {
                    let dc = (dst >> shift) & 0xFF;
                    let sc = (src >> shift) & 0xFF;
                    let exact = (dc * inv + sc * au) / 255;
                    let gc = (got >> shift) & 0xFF;
                    assert!(
                        gc.abs_diff(exact) <= 1,
                        "channel diff too large: alpha={a} got={gc} exact={exact}"
                    );
                }
            }
        }

        // 2. Settings customization: Accent palette, Floating Dock, Animations, Glass Intensity
        let mut desktop = Desktop::new(1160, 720);
        let sw = desktop.windows.last().unwrap().clone(); // Settings at (370, 90, 520, 344)
        assert_eq!(desktop.settings.accent_idx, 0);
        assert!(!desktop.settings.floating_dock);
        assert!(desktop.settings.animations);
        assert_eq!(desktop.settings.glass_level, 0);

        // Click Display tab (sidebar idx 0: x+20, y+45)
        desktop.on_mouse_down(sw.x + 40, sw.y + 45, 1160, 720);
        assert_eq!(desktop.settings.page, apps::SettingsPage::Display);
        // Step animations -> pill smoothly approaches Display tab (y=0.0)
        for _ in 0..40 {
            desktop.on_tick();
        }
        assert!(desktop.settings.tab_pill_y < 0.5);

        // Select Cobalt custom accent dot (index 1 -> ACCENT_PALETTE[0])
        desktop.on_mouse_down(sw.x + sw.w - 136 + 22, sw.y + 230, 1160, 720);
        assert_eq!(desktop.settings.accent_idx, 1);
        assert_eq!(desktop.settings.active_accent(), apps::ACCENT_PALETTE[0]);

        // Click Taskbar tab (sidebar idx 1: y+75) & toggle Floating Minimal Dock
        desktop.on_mouse_down(sw.x + 40, sw.y + 75, 1160, 720);
        assert_eq!(desktop.settings.page, apps::SettingsPage::Taskbar);
        desktop.on_mouse_down(sw.x + 250, sw.y + 110, 1160, 720);
        assert!(desktop.settings.floating_dock);
        for _ in 0..40 {
            desktop.on_tick();
        }
        assert!((desktop.settings.toggle_anim[3] - 1.0).abs() < 0.02);

        // Click System tab (sidebar idx 2: y+105) & cycle Glass Intensity -> Airy (1)
        desktop.on_mouse_down(sw.x + 40, sw.y + 105, 1160, 720);
        assert_eq!(desktop.settings.page, apps::SettingsPage::System);
        desktop.on_mouse_down(sw.x + 250, sw.y + 188, 1160, 720);
        assert_eq!(desktop.settings.glass_level, 1);
        assert_eq!(desktop.settings.glass_alphas().0, 182);

        // Open Start menu on Floating Dock and render full desktop
        desktop.on_mouse_down(30, 700, 1160, 720);
        assert!(desktop.start_menu_open);
        for _ in 0..20 {
            desktop.on_tick();
        }
        assert!(desktop.start_anim > 0.9);
        let mut canvas = gfx::Canvas::new(1160, 720);
        desktop.render(&mut canvas);
        assert!(canvas.pixels.iter().any(|&p| p != 0));
    }

    #[test]
    fn test_macos_genie_minimize_restore_and_window_morph() {
        let mut desktop = Desktop::new(1160, 720);
        let mut canvas = gfx::Canvas::new(1160, 720);
        assert!(desktop.settings.genie_minimize);

        // Hover over top-left desktop icon (Terminal at 18, 42) -> icon_hover[0] rises
        desktop.on_mouse_move(36, 62, 1160, 720);
        for _ in 0..12 {
            desktop.on_tick();
        }
        assert!(desktop.icon_hover[0] > 0.8);

        // 1. Click minimize button [_] on Settings (370, 90, 520, 344) -> win.x + win.w - 62, win.y + 14
        let sw = desktop.windows.last().unwrap().clone();
        desktop.on_mouse_down(sw.x + sw.w - 62, sw.y + 14, 1160, 720);
        assert!(desktop.windows.last().unwrap().minimized);
        // Step halfway through Genie minimize and verify intermediate warp frame renders
        for _ in 0..6 {
            desktop.on_tick();
        }
        let mid_anim = desktop.windows.last().unwrap().min_anim;
        assert!(mid_anim > 0.2 && mid_anim < 0.8, "unexpected mid min_anim: {mid_anim}");
        desktop.render(&mut canvas);
        for _ in 0..15 {
            desktop.on_tick();
        }
        assert_eq!(desktop.windows.last().unwrap().min_anim, 0.0);

        // 2. Restore Settings via taskbar pill -> Genie unminimize smoothly opens back to 1.0
        // Open windows order: System (0), Settings (1) -> pill 1 at x = 102 + pill_w + 4
        desktop.on_mouse_down(250, 700, 1160, 720);
        assert!(!desktop.windows.last().unwrap().minimized);
        for _ in 0..5 {
            desktop.on_tick();
        }
        let restore_mid = desktop.windows.last().unwrap().min_anim;
        assert!(restore_mid > 0.1 && restore_mid < 0.9, "unexpected restore min_anim: {restore_mid}");
        desktop.render(&mut canvas);
        for _ in 0..15 {
            desktop.on_tick();
        }
        assert_eq!(desktop.windows.last().unwrap().min_anim, 1.0);

        // 3. Maximize Settings [□] -> target snaps immediately while vis_rect glides smoothly
        let sw2 = desktop.windows.last().unwrap().clone();
        desktop.on_mouse_down(sw2.x + sw2.w - 39, sw2.y + 14, 1160, 720);
        assert!(desktop.windows.last().unwrap().maximized);
        assert_eq!(desktop.windows.last().unwrap().w, 1160);
        assert!(desktop.windows.last().unwrap().vis_rect[2] < 600.0);
        for _ in 0..25 {
            desktop.on_tick();
        }
        assert!((desktop.windows.last().unwrap().vis_rect[2] - 1160.0).abs() < 1.0);

        // 4. Close Settings [×] -> open becomes false while close_anim fades out
        let max_w = desktop.windows.last().unwrap().clone();
        desktop.on_mouse_down(max_w.x + max_w.w - 16, max_w.y + 14, 1160, 720);
        assert!(!desktop.windows.last().unwrap().open);
        desktop.on_tick();
        let ca = desktop.windows.last().unwrap().close_anim;
        assert!(ca > 0.5 && ca < 1.0);
        desktop.render(&mut canvas);
    }

    #[test]
    fn test_paint_continuous_stroke_display_picker_and_dock_anim() {
        let mut desktop = Desktop::new(1160, 720);

        // 1. PizPaint continuous line interpolation across fast mouse jump
        desktop.paint.paint_at(24, 46);
        desktop.paint.paint_at(124, 86);
        // Midpoint along (24,46) -> (124,86) is (74, 66); canvas offset inside Paint is (14, 36)
        let mid_idx = (66 - 36) as usize * crate::apps::PAINT_W + (74 - 14) as usize;
        assert_ne!(
            desktop.paint.buf[mid_idx], 0x0D1117,
            "midpoint of fast paint drag should be interpolated without gaps"
        );
        // end_stroke() stops connecting subsequent clicks
        desktop.paint.end_stroke();
        desktop.paint.paint_at(24, 126);
        let gap_idx = (106 - 36) as usize * crate::apps::PAINT_W + (24 - 14) as usize;
        assert_eq!(
            desktop.paint.buf[gap_idx], 0x0D1117,
            "after end_stroke(), new stroke must not connect from previous stroke"
        );

        // 2. DisplayPicker popup in Settings -> Экран и язык
        assert!(desktop.settings.display_picker.is_none());
        // Switch to Display tab (idx 0 at x=60, y=46)
        desktop.settings.on_click(60, 46, 520, 344);
        assert_eq!(desktop.settings.page, crate::apps::SettingsPage::Display);
        // Click "Тема и обои" row (lx=220, ly=76)
        desktop.settings.on_click(220, 76, 520, 344);
        assert_eq!(
            desktop.settings.display_picker,
            Some(crate::apps::DisplayPicker::Theme)
        );
        // Select 2nd option (Crimson Sunset) at px = 520 - 192 - 16 = 312, py = 94; row 1 at y = 94 + 6 + 30 + 10 = 140
        desktop.settings.on_click(340, 140, 520, 344);
        assert_eq!(desktop.settings.theme, crate::apps::ThemePreset::CrimsonSunset);
        assert!(desktop.settings.display_picker.is_none());

        // Click "Язык интерфейса" row (lx=220, ly=110)
        desktop.settings.on_click(220, 110, 520, 344);
        assert_eq!(
            desktop.settings.display_picker,
            Some(crate::apps::DisplayPicker::Lang)
        );
        // Select 2nd option (English EN) at px = 520 - 172 - 16 = 332, py = 128; row 1 at y = 128 + 8 + 30 + 10 = 176
        desktop.settings.on_click(360, 176, 520, 344);
        assert_eq!(desktop.settings.lang, crate::apps::Lang::En);
        assert!(desktop.settings.display_picker.is_none());

        // 3. Animated Floating Dock transition
        assert_eq!(desktop.dock_float_anim, 0.0);
        desktop.settings.floating_dock = true;
        for _ in 0..3 {
            desktop.on_tick();
        }
        assert!(
            desktop.dock_float_anim > 0.1 && desktop.dock_float_anim < 0.9,
            "unexpected mid dock_float_anim: {}",
            desktop.dock_float_anim
        );
        for _ in 0..25 {
            desktop.on_tick();
        }
        assert_eq!(desktop.dock_float_anim, 1.0);
    }

    #[test]
    fn test_vector_ui_scaling_and_non_linear_motion() {
        // 1. HiDPI logical-to-physical vector canvas dimensions
        let mut canvas = Canvas::new(1500, 900);
        let mut desktop = Desktop::new(1500, 900);

        desktop.settings.ui_scale_pct = 125;
        canvas.set_ui_scale(desktop.settings.ui_scale());
        assert_eq!(canvas.logical_width(), 1200);
        assert_eq!(canvas.logical_height(), 720);
        desktop.render(&mut canvas);
        assert!(canvas.pixels.iter().any(|&p| p != 0));

        // 2. Keyboard scale delta step + dynamic toast feedback
        desktop.on_key(crate::apps::KeyEvent::ScaleDelta(1));
        assert_eq!(desktop.settings.ui_scale_pct, 150);
        assert!(desktop.toast.is_some());
        canvas.set_ui_scale(desktop.settings.ui_scale());
        assert_eq!(canvas.logical_width(), 1000);
        assert_eq!(canvas.logical_height(), 600);
        desktop.render(&mut canvas);

        desktop.on_key(crate::apps::KeyEvent::ScaleDelta(0));
        assert_eq!(desktop.settings.ui_scale_pct, 100);

        // 3. Non-linear easing & damped spring overshoot/settlement
        assert!(Canvas::ease_out_back(0.65) > 1.0);
        assert!((Canvas::ease_in_out_cubic(0.5) - 0.5).abs() < 1e-5);
        assert!(Canvas::ease_out_cubic(0.25) > 0.25);

        let mut pos = 0.0f32;
        let mut vel = 0.0f32;
        for _ in 0..24 {
            Canvas::step_spring(&mut pos, &mut vel, 1.0, 0.36, 0.52, 0.008);
        }
        assert_eq!(pos, 1.0);
    }

    #[test]
    fn test_snap_zones_control_center_spotlight_and_toast() {
        let mut canvas = Canvas::new(1160, 720);
        let mut desktop = Desktop::new(1160, 720);

        // 1. Window Snap Zone Left via dragging Settings titlebar to left edge
        let sw = desktop.windows.last().unwrap().clone();
        desktop.on_mouse_down(sw.x + 80, sw.y + 12, 1160, 720);
        assert!(desktop.dragging.is_some());
        desktop.on_mouse_move(6, 200, 1160, 720);
        assert_eq!(desktop.snap_zone, Some(crate::desktop::SnapZone::Left));
        desktop.on_tick();
        desktop.render(&mut canvas);

        desktop.on_mouse_up();
        assert!(desktop.dragging.is_none());
        let snapped = desktop.windows.last().unwrap().clone();
        assert_eq!(snapped.x, 6);
        assert!(snapped.w > 500 && snapped.w < 600);
        assert!(desktop.toast.is_some());
        for _ in 0..24 {
            desktop.on_tick();
        }
        assert!((desktop.windows.last().unwrap().vis_rect[0] - 6.0).abs() < 1.0);

        // 2. Spotlight Quick Launcher + live math parser + launching Paint
        assert_eq!(crate::desktop::eval_quick_math("12 * (3 + 1)"), Some(48.0));
        desktop.on_key(crate::apps::KeyEvent::ToggleLauncher);
        assert!(desktop.launcher_open);
        for ch in "12*4".chars() {
            desktop.on_key(crate::apps::KeyEvent::Char(ch));
        }
        desktop.render(&mut canvas);
        desktop.on_key(crate::apps::KeyEvent::Enter);
        assert!(!desktop.launcher_open);
        assert_eq!(desktop.calc.display, "48");

        desktop.on_key(crate::apps::KeyEvent::ToggleLauncher);
        for ch in "paint".chars() {
            desktop.on_key(crate::apps::KeyEvent::Char(ch));
        }
        desktop.on_key(crate::apps::KeyEvent::Enter);
        assert_eq!(
            desktop.windows.last().unwrap().id,
            crate::desktop::AppId::Paint
        );

        // 3. Control Center taskbar pill (on 1160px: lang_x = 1016, cc_x = 986..1011)
        desktop.on_mouse_down(998, 700, 1160, 720);
        assert!(desktop.cc_open);
        for _ in 0..6 {
            desktop.on_tick();
        }
        desktop.render(&mut canvas);
        assert!(desktop.cc_anim > 0.3);
        // Click UI Scale 125% inside Control Center (ph = 298 -> py = 720 - 36 - 298 - 6 = 380, scale_y = 380 + 170 = 550)
        desktop.on_mouse_down(965, 562, 1160, 720);
        assert_eq!(desktop.settings.ui_scale_pct, 125);
    }

    #[test]
    fn test_paint_tools_and_piznake_upgrades() {
        let mut desktop = Desktop::new(1160, 720);

        // 1. PizPaint size cycle, Flood Fill, Eraser & BMP export
        desktop.focus_or_open(crate::desktop::AppId::Paint);
        let p_wh = 268;
        // Pick color index 1 (red) & cycle brush size from M(4) -> L(7) -> S(2)
        desktop.paint.on_click(38, p_wh - 20, p_wh);
        desktop.paint.on_click(170, p_wh - 20, p_wh);
        assert_eq!(desktop.paint.brush, 7);
        desktop.paint.on_click(170, p_wh - 20, p_wh);
        assert_eq!(desktop.paint.brush, 2);
        // Enable Flood Fill and click inside canvas
        desktop.paint.on_click(260, p_wh - 20, p_wh);
        assert!(desktop.paint.fill_mode);
        desktop.paint.on_click(40, 60, p_wh);
        desktop.paint.end_stroke();
        assert_ne!(desktop.paint.buf[0], 0x0D1117);
        // Save BMP and clean up temp file
        if let crate::apps::PaintAction::Saved(fname) = desktop.paint.on_click(300, p_wh - 20, p_wh)
        {
            let tmp = std::env::temp_dir().join(&fname);
            assert!(tmp.exists());
            let _ = std::fs::remove_file(tmp);
        } else {
            panic!("expected PaintAction::Saved");
        }
        // Enable Eraser and clear canvas pixel
        desktop.paint.on_click(210, p_wh - 20, p_wh);
        assert!(desktop.paint.eraser);
        desktop.paint.on_click(40, 60, p_wh);
        let idx = (60 - 36) as usize * crate::apps::PAINT_W + (40 - 14) as usize;
        assert_eq!(desktop.paint.buf[idx], 0x0D1117);

        // 2. Pizмейка / Piznake portal walls wrap & interpolation
        assert!(desktop.snake.wrap_walls);
        desktop.snake.on_click(140, 278 - 20, 278);
        assert!(!desktop.snake.wrap_walls);
        desktop.snake.on_click(140, 278 - 20, 278);
        assert!(desktop.snake.wrap_walls);
        desktop.snake.body = vec![(0, 5), (1, 5), (2, 5)];
        desktop.snake.prev_body = desktop.snake.body.clone();
        desktop.snake.dir = (-1, 0);
        desktop.snake.step();
        assert!(!desktop.snake.game_over);
        assert_eq!(desktop.snake.body[0].0, 19);
        desktop.snake.step_interp(8, true);
        assert!(desktop.snake.move_t > 0.0);
    }

    #[test]
    fn test_calc_oxygenos_never_settle_easter_egg() {
        let mut desktop = Desktop::new(1160, 720);
        let mut canvas = gfx::Canvas::new(1160, 720);
        desktop.focus_or_open(crate::desktop::AppId::Calc);

        // Keyboard sequence: 1 + = -> Never Settle
        desktop.on_key(apps::KeyEvent::Char('1'));
        desktop.on_key(apps::KeyEvent::Char('+'));
        desktop.on_key(apps::KeyEvent::Char('='));
        assert_eq!(desktop.calc.display, "Never Settle");
        assert!(desktop.calc.never_settle);
        desktop.render(&mut canvas);

        // Backspace clears easter egg state cleanly back to "0"
        desktop.on_key(apps::KeyEvent::Backspace);
        assert_eq!(desktop.calc.display, "0");
        assert!(!desktop.calc.never_settle);

        // Normal arithmetic 1 + 1 = 2 must not trigger easter egg
        desktop.calc.press("1");
        desktop.calc.press("+");
        desktop.calc.press("1");
        desktop.calc.press("=");
        assert_eq!(desktop.calc.display, "2");
        assert!(!desktop.calc.never_settle);

        // Button press sequence 1 + = triggers Never Settle, and "C" clears it
        desktop.calc.press("1");
        desktop.calc.press("+");
        desktop.calc.press("=");
        assert_eq!(desktop.calc.display, "Never Settle");
        assert!(desktop.calc.never_settle);
        desktop.calc.press("C");
        assert_eq!(desktop.calc.display, "0");
        assert!(!desktop.calc.never_settle);
    }

    #[test]
    fn test_always_on_dynamic_island_persistent_mode() {
        let mut desktop = Desktop::new(1160, 720);
        let mut canvas = gfx::Canvas::new(1160, 720);
        assert!(!desktop.settings.persistent_island);
        assert!(desktop.toast.is_none());

        // Enable Always-On Dynamic Island in Settings -> Taskbar tab (row 264..298)
        let sw = desktop.windows.last().unwrap().clone();
        desktop.on_mouse_down(sw.x + 40, sw.y + 75, 1160, 720);
        assert_eq!(desktop.settings.page, apps::SettingsPage::Taskbar);
        desktop.on_mouse_down(sw.x + 220, sw.y + 278, 1160, 720);
        assert!(desktop.settings.persistent_island);

        for _ in 0..32 {
            desktop.on_tick();
        }
        assert_eq!(desktop.island_anim, 1.0);
        assert!((desktop.settings.toggle_anim[8] - 1.0).abs() < 0.02);
        desktop.render(&mut canvas);

        // Clicking the persistent Dynamic Island pill at top-center toggles Control Center
        desktop.on_mouse_down(580, 22, 1160, 720);
        assert!(desktop.cc_open);

        // Toggle Always-On Dynamic Island back off inside Control Center (py = 380, island_y = 582..608)
        desktop.on_mouse_down(960, 592, 1160, 720);
        assert!(!desktop.settings.persistent_island);
    }

    #[test]
    fn test_vista_sidebar_aero_stars_night_shift_crt_and_config() {
        let mut desktop = Desktop::new(1160, 720);
        let mut canvas = gfx::Canvas::new(1160, 720);
        assert!(desktop.settings.vista_sidebar);
        assert!(desktop.settings.aero_stars);

        // Focus sticky note in Vista Sidebar (x = 1160 - 176 = 984, y = 272), type text, save & reload config
        desktop.on_mouse_down(1020, 300, 1160, 720);
        assert!(desktop.sticky_focused);
        desktop.on_key(apps::KeyEvent::Char('!'));
        desktop.on_key(apps::KeyEvent::Escape);
        assert!(!desktop.sticky_focused);

        desktop.settings.night_shift = true;
        desktop.settings.crt_scanlines = true;
        for _ in 0..5 {
            desktop.on_tick();
        }
        desktop.render(&mut canvas);
        assert!(canvas.pixels.iter().any(|&p| p != 0));

        // Config round-trip on temp file
        let tmp = std::env::temp_dir().join("pizsoft_test_cfg.conf");
        config::save_config_to(&tmp, &desktop.settings, 42, "Hi\\nthere");
        let mut reloaded = apps::SettingsState::new();
        let persisted = config::load_config_from(&tmp, &mut reloaded);
        let _ = std::fs::remove_file(&tmp);
        assert!(reloaded.night_shift);
        assert!(reloaded.crt_scanlines);
        assert_eq!(persisted.best_snake, 42);
    }

    #[test]
    fn test_offscreen_canvas_clipping_and_drag_no_panic() {
        for pct in apps::UI_SCALES {
            let mut canvas = Canvas::new(800, 600);
            canvas.set_ui_scale(pct as f32 / 100.0);
            let lw = canvas.logical_width();
            let lh = canvas.logical_height();

            for &(x, y, w, h) in &[
                (lw, 20, 120, 80),
                (lw + 200, lh / 2, 160, 90),
                (40, lh, 180, 60),
                (40, lh + 150, 180, 60),
                (-300, -200, 120, 80),
                (0, 0, 0, 0),
                (lw - 2, lh - 2, 1, 1),
            ] {
                canvas.fill_rounded_rect(x, y, w, h, 10, 0x336699);
                canvas.blend_rounded_rect(x, y, w, h, 10, 0x99CCFF, 120);
                canvas.draw_rounded_rect_alpha(x, y, w, h, 10, 0xFFFFFF, 90);
                canvas.fill_luminous_card(x, y, w, h, 10, 0x112233, 0x223344, 0x38BDF8, 0xA855F7);
                canvas.draw_soft_shadow(x, y, w, h, 10);
                canvas.draw_button(x, y, w, h, "OK", 0x1E293B, 0xFFFFFF, 0x64748B);
            }
            canvas.fill_aa_circle((lw + 40) as f32, 30.0, 25.0, 0xFF0000, 200);
            canvas.draw_aa_line((lw + 5) as f32, -10.0, (lw + 90) as f32, (lh + 40) as f32, 3.0, 0xFFFFFF, 255);
            canvas.fill_aa_triangle((lw + 2) as f32, 0.0, (lw + 50) as f32, 40.0, (lw - 10) as f32, 50.0, 0x00FF00, 180);

            // Drag desktop window off the far right/bottom edges and render without panics
            let mut desktop = Desktop::new(lw, lh);
            desktop.settings.ui_scale_pct = pct;
            let sw = desktop.windows.last().unwrap().clone();
            desktop.on_mouse_down(sw.x + 80, sw.y + 12, lw, lh);
            desktop.on_mouse_move(lw + 400, lh + 300, lw, lh);
            desktop.on_tick();
            desktop.render(&mut canvas);
            desktop.on_mouse_up();
        }
    }

    #[test]
    fn test_stage2_game2048_switcher_lock_and_island() {
        let mut desktop = Desktop::new(1160, 720);
        let mut canvas = gfx::Canvas::new(1160, 720);

        // Open 2048, play moves, verify board updates & score progresses
        desktop.focus_or_open(desktop::AppId::Game2048);
        assert!(desktop.windows.last().unwrap().id == desktop::AppId::Game2048);
        desktop.on_key(apps::KeyEvent::Left);
        desktop.on_key(apps::KeyEvent::Down);
        desktop.on_key(apps::KeyEvent::Right);
        desktop.on_key(apps::KeyEvent::Up);
        assert!(desktop.game2048.grid.iter().flatten().any(|&v| v > 0));

        // Window Switcher (Alt+Tab / Ctrl+Tab) opens, cycles, and Enter switches
        desktop.focus_or_open(desktop::AppId::Terminal);
        desktop.on_key(apps::KeyEvent::SwitchWindows);
        assert!(desktop.switcher_open);
        desktop.render(&mut canvas);
        desktop.on_key(apps::KeyEvent::Enter);
        assert!(!desktop.switcher_open);

        // DND blocks normal push_toast, while Island cycles Status -> Music -> Pomodoro -> Status
        desktop.dnd = true;
        desktop.toast = None;
        desktop.push_toast("Suppressed", 0xFFFFFF);
        assert!(desktop.toast.is_none());
        desktop.cycle_island_mode();
        assert_eq!(desktop.island_mode, desktop::IslandMode::Music);
        desktop.render(&mut canvas);
        desktop.cycle_island_mode();
        assert_eq!(desktop.island_mode, desktop::IslandMode::Pomodoro);
        desktop.render(&mut canvas);
        desktop.cycle_island_mode();
        assert_eq!(desktop.island_mode, desktop::IslandMode::Status);

        // Lock screen engages on Ctrl+L and unlocks on Space/click
        desktop.on_key(apps::KeyEvent::LockScreen);
        assert!(desktop.locked);
        desktop.render(&mut canvas);
        desktop.on_key(apps::KeyEvent::Char(' '));
        assert!(!desktop.locked);
    }

    #[test]
    fn test_stage3_files_pizview_minesweeper_workspaces_and_clipboard() {
        let mut desktop = Desktop::new(1160, 720);
        let mut canvas = gfx::Canvas::new(1160, 720);

        // 1. Terminal autocomplete, history, pizfetch, theme command
        desktop.focus_or_open(desktop::AppId::Terminal);
        for c in "pizf".chars() {
            desktop.on_key(apps::KeyEvent::Char(c));
        }
        desktop.on_key(apps::KeyEvent::Tab);
        assert_eq!(desktop.terminal.input, "pizfetch");
        desktop.on_key(apps::KeyEvent::Enter);
        assert!(desktop.terminal.history.iter().any(|h| h == "pizfetch"));
        desktop.on_key(apps::KeyEvent::Up);
        assert_eq!(desktop.terminal.input, "pizfetch");
        desktop.terminal.input.clear();
        for c in "theme 2".chars() {
            desktop.on_key(apps::KeyEvent::Char(c));
        }
        desktop.on_key(apps::KeyEvent::Enter);
        assert_eq!(desktop.settings.theme, apps::ThemePreset::MatrixGlow);

        // 2. Paint Undo + Eyedropper + Shape cycle
        desktop.focus_or_open(desktop::AppId::Paint);
        let pw = desktop.windows.last().unwrap().clone();
        let before_px = desktop.paint.buf[30 * apps::PAINT_W + 30];
        desktop.paint.paint_at(14 + 30, 36 + 30);
        desktop.paint.end_stroke();
        assert_ne!(desktop.paint.buf[30 * apps::PAINT_W + 30], before_px);
        desktop.on_key(apps::KeyEvent::Undo);
        assert_eq!(desktop.paint.buf[30 * apps::PAINT_W + 30], before_px);
        let _ = desktop.paint.on_click(90, pw.h - 50, pw.h);
        assert!(desktop.paint.eyedropper);
        let _ = desktop.paint.on_click(150, pw.h - 50, pw.h);
        assert_eq!(desktop.paint.shape, apps::PaintShape::Line);

        // 3. Notepad typing, undo, clipboard Copy/Paste, Find
        desktop.focus_or_open(desktop::AppId::Notepad);
        let line_count_before = desktop.notepad.lines.len();
        desktop.on_key(apps::KeyEvent::Char('X'));
        assert!(desktop.notepad.dirty);
        assert!(desktop.notepad.lines.last().unwrap().ends_with('X'));
        desktop.on_key(apps::KeyEvent::Copy);
        let clip = platform::paste_from_clipboard().expect("clipboard text");
        assert!(clip.contains('X'));
        desktop.on_key(apps::KeyEvent::Undo);
        desktop.on_key(apps::KeyEvent::Paste(String::from("Y")));
        assert!(desktop.notepad.lines.last().unwrap().ends_with('Y'));
        desktop.on_key(apps::KeyEvent::Find);
        assert!(desktop.notepad.search_mode);
        assert_eq!(desktop.notepad.lines.len(), line_count_before);

        // 4. Calc Sci strip (x^2) while keeping 1+= Easter Egg
        desktop.focus_or_open(desktop::AppId::Calc);
        desktop.calc.press("3");
        desktop.calc.on_click(30, 88, 250, 300); // x^2
        assert_eq!(desktop.calc.display, "9");
        desktop.calc.press("C");
        desktop.calc.press("1");
        desktop.calc.press("+");
        desktop.calc.press("=");
        assert!(desktop.calc.never_settle);

        // 5. Files CRUD + open .bmp in PizView + open .txt in Notepad
        let tmp_dir = std::env::temp_dir().join("pizsoft_stage3_files_test");
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let shot_bmp = tmp_dir.join("sample.bmp");
        desktop.render(&mut canvas);
        canvas.write_bmp(&shot_bmp).unwrap();

        desktop.cwd = tmp_dir.clone();
        desktop.reload_files();
        desktop.focus_or_open(desktop::AppId::Files);
        let fw = desktop.windows.last().unwrap().clone();
        // [+File] button at win.x + 130, win.y + 60
        desktop.on_mouse_down(fw.x + 130, fw.y + 60, 1160, 720);
        desktop.on_mouse_up();
        assert!(tmp_dir.join("note_1.txt").exists());
        // [Sort] button
        desktop.on_mouse_down(fw.x + 75, fw.y + 60, 1160, 720);
        desktop.on_mouse_up();
        assert!(desktop.file_sort_desc);
        // Click row for sample.bmp -> opens PizView
        let bmp_row = desktop.files.iter().position(|f| f.name == "sample.bmp").unwrap();
        desktop.on_mouse_down(fw.x + 40, fw.y + 76 + (bmp_row as i32) * 18 + 6, 1160, 720);
        desktop.on_mouse_up();
        assert_eq!(desktop.windows.last().unwrap().id, desktop::AppId::PizView);
        assert_eq!(desktop.pizview.img_w, 1160);
        let vw = desktop.windows.last().unwrap().clone();
        desktop.pizview.on_click(40, vw.h - 20, vw.h); // Rotate
        desktop.pizview.on_click(130, vw.h - 20, vw.h); // Zoom 2x
        assert_eq!(desktop.pizview.rot90, 1);
        assert!(desktop.pizview.zoom2x);
        // Click note_1.txt -> opens Notepad
        desktop.focus_or_open(desktop::AppId::Files);
        let note_row = desktop.files.iter().position(|f| f.name == "note_1.txt").unwrap();
        desktop.on_mouse_down(fw.x + 40, fw.y + 76 + (note_row as i32) * 18 + 6, 1160, 720);
        desktop.on_mouse_up();
        assert_eq!(desktop.windows.last().unwrap().id, desktop::AppId::Notepad);
        assert!(desktop.notepad.file_path.as_ref().is_some_and(|p| p.ends_with("note_1.txt")));
        // Delete note_1.txt via [Del]
        desktop.focus_or_open(desktop::AppId::Files);
        let del_row = desktop.files.iter().position(|f| f.name == "note_1.txt").unwrap();
        desktop.file_sel = del_row;
        desktop.on_mouse_down(fw.x + 245, fw.y + 60, 1160, 720);
        desktop.on_mouse_up();
        assert!(!tmp_dir.join("note_1.txt").exists());
        let _ = std::fs::remove_dir_all(&tmp_dir);

        // 6. Minesweeper safe first reveal + right-click flag
        desktop.focus_or_open(desktop::AppId::Minesweeper);
        let mw = desktop.windows.last().unwrap().clone();
        desktop.on_mouse_down(mw.x + 45, mw.y + 85, 1160, 720);
        desktop.on_mouse_up();
        assert!(desktop.minesweeper.started);
        assert!(!desktop.minesweeper.lost);
        desktop.on_right_click(mw.x + mw.w - 30, mw.y + mw.h - 50, 1160, 720);
        desktop.ctx_menu = None;

        // 7. Workspaces & Show Desktop
        desktop.on_key(apps::KeyEvent::Workspace(2));
        assert_eq!(desktop.active_workspace, 2);
        desktop.focus_or_open(desktop::AppId::Calc);
        assert_eq!(desktop.windows.last().unwrap().workspace, 2);
        desktop.on_key(apps::KeyEvent::ShowDesktop);
        assert!(desktop.windows.last().unwrap().minimized);
        desktop.on_key(apps::KeyEvent::ShowDesktop);
        assert!(!desktop.windows.last().unwrap().minimized);
        desktop.on_key(apps::KeyEvent::Workspace(1));
        assert_eq!(desktop.active_workspace, 1);

        // 8. Calendar popup + Notification History Clear + macOS Buttons + Reset Defaults
        desktop.on_mouse_down(941, 700, 1160, 720);
        desktop.on_mouse_up();
        assert!(desktop.cal_open);
        assert!(!desktop.toast_history.is_empty());
        desktop.render(&mut canvas);
        desktop.on_mouse_down(1120, 720 - 36 - 310 - 8 + 162, 1160, 720);
        desktop.on_mouse_up();
        assert!(desktop.toast_history.is_empty());
        desktop.cal_open = false;

        desktop.focus_or_open(desktop::AppId::Settings);
        desktop.settings.page = apps::SettingsPage::Taskbar;
        desktop.settings.on_click(260, 310, 520, 344); // toggle macos_buttons
        assert!(desktop.settings.macos_buttons);
        let sw = desktop.windows.last().unwrap().clone();
        desktop.on_mouse_down(sw.x + 49, sw.y + 14, 1160, 720); // green macOS traffic light -> maximize
        desktop.on_mouse_up();
        assert!(desktop.windows.last().unwrap().maximized);

        let maxw = desktop.windows.last().unwrap().clone();
        desktop.settings.page = apps::SettingsPage::System;
        desktop.on_mouse_down(maxw.x + 240, maxw.y + 295, 1160, 720); // ResetDefaults
        desktop.on_mouse_up();
        assert!(!desktop.settings.macos_buttons);
        assert_eq!(desktop.settings.theme, apps::ThemePreset::AuroraPeaks);
        desktop.render(&mut canvas);
    }

    #[test]
    fn test_x11_display_server_guest_wm_and_files_icon() {
        use std::io::{Read, Write};
        use std::net::TcpStream;
        use std::time::Duration;

        let mut canvas = Canvas::new(1160, 720);
        let mut desktop = Desktop::new(1160, 720);
        assert_eq!(desktop.icon_hover.len(), 13);

        // Minimize all open windows and test clicking the new desktop "Файлы / Files" icon
        for w in &mut desktop.windows {
            w.open = false;
        }
        desktop.on_mouse_down(102 + 20, 402 + 20, 1160, 720);
        desktop.on_mouse_up();
        assert_eq!(desktop.windows.last().unwrap().id, desktop::AppId::Files);
        assert!(desktop.windows.last().unwrap().open);

        // Verify x11demo builtin & autocomplete in Terminal
        desktop.terminal.input = String::from("x11d");
        desktop.terminal.on_key(apps::KeyEvent::Tab, &desktop.kernel, &desktop.mem);
        assert_eq!(desktop.terminal.input, "x11demo");
        let act = desktop.terminal.on_key(apps::KeyEvent::Enter, &desktop.kernel, &desktop.mem);
        assert!(matches!(act, apps::TermAction::LaunchX11Demo));

        // Start real loopback X11 TCP server and attach it to Desktop
        let srv = display_srv::DisplayServer::new_tcp_test(0).expect("bind test x11 tcp");
        let port = srv.tcp_port.expect("x11 tcp port");
        desktop.display_srv = srv;

        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect x11");
        stream.set_nonblocking(true).unwrap();

        // X11 setup handshake (LSB first, protocol 11.0)
        let mut setup = [0u8; 12];
        setup[0] = b'l';
        setup[2..4].copy_from_slice(&11u16.to_le_bytes());
        stream.write_all(&setup).unwrap();

        let mut setup_rep = vec![0u8; 256];
        let mut got_setup = 0usize;
        for _ in 0..40 {
            desktop.poll_display_server();
            if let Ok(n) = stream.read(&mut setup_rep[got_setup..]) {
                got_setup += n;
                if got_setup >= 140 {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(got_setup >= 32, "expected X11 setup reply");
        assert_eq!(setup_rep[0], 1, "expected X11 Setup Success");

        let wid: u32 = 0x0040_0001;
        let gc: u32 = 0x0040_0002;
        let root = u32::from_le_bytes(setup_rep[68..72].try_into().unwrap());
        assert_eq!(root, 0x0000_0100);

        // CreateWindow (wid, 160x120)
        let mut cw = [0u8; 32];
        cw[0] = 1;
        cw[1] = 24;
        cw[2..4].copy_from_slice(&8u16.to_le_bytes());
        cw[4..8].copy_from_slice(&wid.to_le_bytes());
        cw[8..12].copy_from_slice(&root.to_le_bytes());
        cw[16..18].copy_from_slice(&160u16.to_le_bytes());
        cw[18..20].copy_from_slice(&120u16.to_le_bytes());
        cw[24..26].copy_from_slice(&1u16.to_le_bytes());
        stream.write_all(&cw).unwrap();

        // ChangeProperty WM_NAME = "TestX11Win"
        let title = b"TestX11Win";
        let words = 6 + title.len().div_ceil(4);
        let mut cp = vec![0u8; words * 4];
        cp[0] = 18;
        cp[2..4].copy_from_slice(&(words as u16).to_le_bytes());
        cp[4..8].copy_from_slice(&wid.to_le_bytes());
        cp[8..12].copy_from_slice(&39u32.to_le_bytes());
        cp[12..16].copy_from_slice(&31u32.to_le_bytes());
        cp[16] = 8;
        cp[20..24].copy_from_slice(&(title.len() as u32).to_le_bytes());
        cp[24..24 + title.len()].copy_from_slice(title);
        stream.write_all(&cp).unwrap();

        // CreateGC
        let mut cg = [0u8; 16];
        cg[0] = 55;
        cg[2..4].copy_from_slice(&4u16.to_le_bytes());
        cg[4..8].copy_from_slice(&gc.to_le_bytes());
        cg[8..12].copy_from_slice(&wid.to_le_bytes());
        stream.write_all(&cg).unwrap();

        // ChangeGC Foreground = 0x0038BDF8
        let mut chg = [0u8; 16];
        chg[0] = 56;
        chg[2..4].copy_from_slice(&4u16.to_le_bytes());
        chg[4..8].copy_from_slice(&gc.to_le_bytes());
        chg[8..12].copy_from_slice(&4u32.to_le_bytes());
        chg[12..16].copy_from_slice(&0x0038_BDF8u32.to_le_bytes());
        stream.write_all(&chg).unwrap();

        // MapWindow
        let mut mw = [0u8; 8];
        mw[0] = 8;
        mw[2..4].copy_from_slice(&2u16.to_le_bytes());
        mw[4..8].copy_from_slice(&wid.to_le_bytes());
        stream.write_all(&mw).unwrap();

        // PolyFillRectangle (10,10,40,30)
        let mut pfr = [0u8; 20];
        pfr[0] = 70;
        pfr[2..4].copy_from_slice(&5u16.to_le_bytes());
        pfr[4..8].copy_from_slice(&wid.to_le_bytes());
        pfr[8..12].copy_from_slice(&gc.to_le_bytes());
        pfr[12..14].copy_from_slice(&10u16.to_le_bytes());
        pfr[14..16].copy_from_slice(&10u16.to_le_bytes());
        pfr[16..18].copy_from_slice(&40u16.to_le_bytes());
        pfr[18..20].copy_from_slice(&30u16.to_le_bytes());
        stream.write_all(&pfr).unwrap();

        // PutImage ZPixmap 2x2 at (2, 2) with pixel 0x00FF00AA
        let mut pi = vec![0u8; 24 + 16];
        pi[0] = 72;
        pi[1] = 2; // ZPixmap
        pi[2..4].copy_from_slice(&10u16.to_le_bytes());
        pi[4..8].copy_from_slice(&wid.to_le_bytes());
        pi[8..12].copy_from_slice(&gc.to_le_bytes());
        pi[12..14].copy_from_slice(&2u16.to_le_bytes());
        pi[14..16].copy_from_slice(&2u16.to_le_bytes());
        pi[16..18].copy_from_slice(&2u16.to_le_bytes());
        pi[18..20].copy_from_slice(&2u16.to_le_bytes());
        pi[21] = 24;
        for chunk in pi[24..].as_chunks_mut::<4>().0 {
            chunk.copy_from_slice(&0x00FF_00AAu32.to_le_bytes());
        }
        stream.write_all(&pi).unwrap();

        let mut gid = None;
        for _ in 0..40 {
            desktop.poll_display_server();
            if let Some(w) = desktop.windows.iter().find(|w| matches!(w.id, desktop::AppId::Guest(_))) {
                gid = Some(w.id);
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let guest_id = gid.expect("Guest window should map into Desktop WM");
        assert_eq!(desktop.win_title(guest_id), "TestX11Win");
        let desktop::AppId::Guest(raw_gid) = guest_id else {
            unreachable!();
        };
        let surf = desktop.display_srv.window(raw_gid).expect("guest surface");
        assert_eq!(surf.pixels[2 * 160 + 2], 0x00FF_00AA);
        assert_eq!(surf.pixels[12 * 160 + 12], 0x0038_BDF8);
        desktop.render(&mut canvas);

        // Drain Expose event then test pointer click + close [×] -> WM_DELETE_WINDOW
        let mut ev = [0u8; 256];
        let mut seen_expose = false;
        for _ in 0..10 {
            if let Ok(n) = stream.read(&mut ev)
                && ev[..n].as_chunks::<32>().0.iter().any(|e| e[0] == 12)
            {
                seen_expose = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(seen_expose, "expected X11 Expose event");

        let gw = desktop.windows.iter().find(|w| w.id == guest_id).unwrap().clone();
        desktop.on_mouse_down(gw.x + gw.w - 16, gw.y + 12, 1160, 720);
        desktop.on_mouse_up();
        assert!(!desktop.windows.iter().any(|w| w.id == guest_id));

        let mut seen_delete = false;
        for _ in 0..20 {
            desktop.poll_display_server();
            if let Ok(n) = stream.read(&mut ev)
                && ev[..n].as_chunks::<32>().0.iter().any(|e| e[0] == 33)
            {
                seen_delete = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(seen_delete, "expected WM_DELETE_WINDOW ClientMessage");
    }

    #[test]
    fn test_screen_resolution_default_800x600_switch_and_persist() {
        let mut d = Desktop::new(800, 600);
        assert_eq!(d.settings.resolution(), (800, 600));
        assert_eq!(d.settings.res_idx, 0);

        // Settings window should fit cleanly within 800x600 width
        let sw = d
            .windows
            .iter()
            .find(|w| w.id == desktop::AppId::Settings)
            .expect("settings win");
        assert!(sw.x + sw.w <= 800);

        // Switch to Display page (sidebar idx 0) then click 1024x768 pill
        let _ = d.settings.on_click(40, 42, sw.w, sw.h);
        let _ = d.settings.on_click(sw.w - 170, 324, sw.w, sw.h);
        assert_eq!(d.settings.resolution(), (1024, 768));

        // Terminal `res 1280x800` command
        let act = d.terminal.on_key(apps::KeyEvent::Char('r'), &d.kernel, &d.mem);
        assert_eq!(act, apps::TermAction::None);
        d.terminal.input = String::from("res 1280x800");
        let act = d.terminal.on_key(apps::KeyEvent::Enter, &d.kernel, &d.mem);
        assert_eq!(act, apps::TermAction::SetResolution(2));
        d.settings.res_idx = 2;
        assert_eq!(d.settings.resolution(), (1280, 800));

        // Config save + reload preserves resolution index
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|x| x.as_nanos())
            .unwrap_or(1);
        let tmp = std::env::temp_dir().join(format!("pizsoft_res_{nanos}.conf"));
        config::save_config_to(&tmp, &d.settings, 7, "note");
        let mut loaded = apps::SettingsState::new();
        assert_eq!(loaded.resolution(), (800, 600));
        let _ = config::load_config_from(&tmp, &mut loaded);
        assert_eq!(loaded.resolution(), (1280, 800));
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn test_x11_child_widgets_drawing_and_xinput2() {
        use std::io::{Read, Write};
        use std::net::TcpStream;
        use std::time::Duration;

        let mut desktop = Desktop::new(800, 600);
        desktop.display_srv = display_srv::DisplayServer::new_tcp_test(0).expect("tcp test listener");
        desktop.display_srv.set_screen_size(800, 600);
        let port = desktop.display_srv.tcp_port.expect("ephemeral port");

        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect x11");
        stream.set_read_timeout(Some(Duration::from_millis(250))).unwrap();

        // Setup handshake -> check 800x600 root screen geometry
        let mut setup = [0u8; 12];
        setup[0] = b'l';
        setup[2..4].copy_from_slice(&11u16.to_le_bytes());
        stream.write_all(&setup).unwrap();
        desktop.poll_display_server();
        let mut reply = [0u8; 256];
        let n = stream.read(&mut reply).unwrap();
        assert!(n >= 140 && reply[0] == 1);
        assert_eq!(u16::from_le_bytes([reply[88], reply[89]]), 800);
        assert_eq!(u16::from_le_bytes([reply[90], reply[91]]), 600);

        // QueryExtension("XInputExtension")
        let ext = b"XInputExtension";
        let words = 2 + ext.len().div_ceil(4);
        let mut qe = vec![0u8; words * 4];
        qe[0] = 98;
        qe[2..4].copy_from_slice(&(words as u16).to_le_bytes());
        qe[4..6].copy_from_slice(&(ext.len() as u16).to_le_bytes());
        qe[8..8 + ext.len()].copy_from_slice(ext);
        stream.write_all(&qe).unwrap();
        desktop.poll_display_server();
        let mut r32b = [0u8; 32];
        stream.read_exact(&mut r32b).unwrap();
        assert_eq!(r32b[8], 1, "XInputExtension must be present");
        let xi_op = r32b[9];
        assert_eq!(xi_op, 131);

        // XIQueryVersion 2.3
        let mut xiqv = [0u8; 8];
        xiqv[0] = xi_op;
        xiqv[1] = 47;
        xiqv[2..4].copy_from_slice(&2u16.to_le_bytes());
        xiqv[4..6].copy_from_slice(&2u16.to_le_bytes());
        xiqv[6..8].copy_from_slice(&3u16.to_le_bytes());
        stream.write_all(&xiqv).unwrap();
        desktop.poll_display_server();
        stream.read_exact(&mut r32b).unwrap();
        assert_eq!(u16::from_le_bytes([r32b[8], r32b[9]]), 2);

        // XIQueryDevice (0 = all)
        let mut xiqd = [0u8; 8];
        xiqd[0] = xi_op;
        xiqd[1] = 48;
        xiqd[2..4].copy_from_slice(&2u16.to_le_bytes());
        stream.write_all(&xiqd).unwrap();
        desktop.poll_display_server();
        let n = stream.read(&mut reply).unwrap();
        assert!(n > 32 && reply[0] == 1);
        assert_eq!(u16::from_le_bytes([reply[8], reply[9]]), 2);

        // Create top window (200x160, white bg) + child button (20,20, 44x28, gray bg)
        let top_wid = 0x0040_0010u32;
        let child_wid = 0x0040_0011u32;
        let gc = 0x0040_0012u32;
        let mut cw = [0u8; 36];
        cw[0] = 1;
        cw[1] = 24;
        cw[2..4].copy_from_slice(&9u16.to_le_bytes());
        cw[4..8].copy_from_slice(&top_wid.to_le_bytes());
        cw[8..12].copy_from_slice(&0x100u32.to_le_bytes());
        cw[16..18].copy_from_slice(&200u16.to_le_bytes());
        cw[18..20].copy_from_slice(&160u16.to_le_bytes());
        cw[24..26].copy_from_slice(&1u16.to_le_bytes());
        cw[28..32].copy_from_slice(&0x02u32.to_le_bytes()); // CWBackPixel
        cw[32..36].copy_from_slice(&0x00FF_FFFFu32.to_le_bytes());
        stream.write_all(&cw).unwrap();

        let mut cwc = [0u8; 40];
        cwc[0] = 1;
        cwc[1] = 24;
        cwc[2..4].copy_from_slice(&10u16.to_le_bytes());
        cwc[4..8].copy_from_slice(&child_wid.to_le_bytes());
        cwc[8..12].copy_from_slice(&top_wid.to_le_bytes());
        cwc[12..14].copy_from_slice(&20u16.to_le_bytes());
        cwc[14..16].copy_from_slice(&20u16.to_le_bytes());
        cwc[16..18].copy_from_slice(&44u16.to_le_bytes());
        cwc[18..20].copy_from_slice(&28u16.to_le_bytes());
        cwc[20..22].copy_from_slice(&1u16.to_le_bytes()); // border_width=1
        cwc[24..26].copy_from_slice(&1u16.to_le_bytes());
        cwc[28..32].copy_from_slice(&0x0Au32.to_le_bytes()); // BackPixel | BorderPixel
        cwc[32..36].copy_from_slice(&0x00C0_C0C0u32.to_le_bytes());
        cwc[36..40].copy_from_slice(&0x0010_2030u32.to_le_bytes());
        stream.write_all(&cwc).unwrap();

        // CreateGC (fg = 0x0011_2233)
        let mut cg = [0u8; 20];
        cg[0] = 55;
        cg[2..4].copy_from_slice(&5u16.to_le_bytes());
        cg[4..8].copy_from_slice(&gc.to_le_bytes());
        cg[8..12].copy_from_slice(&top_wid.to_le_bytes());
        cg[12..16].copy_from_slice(&4u32.to_le_bytes());
        cg[16..20].copy_from_slice(&0x0011_2233u32.to_le_bytes());
        stream.write_all(&cg).unwrap();

        // MapWindow(top_wid) + MapSubwindows(top_wid)
        for &(op, id) in &[(8u8, top_wid), (9u8, top_wid)] {
            let mut mw = [0u8; 8];
            mw[0] = op;
            mw[2..4].copy_from_slice(&2u16.to_le_bytes());
            mw[4..8].copy_from_slice(&id.to_le_bytes());
            stream.write_all(&mw).unwrap();
        }

        // PolyFillArc on top_wid (80, 30, 40x40 full circle) + PolyText8 on child_wid ("7")
        let mut pfa = [0u8; 24];
        pfa[0] = 71;
        pfa[2..4].copy_from_slice(&6u16.to_le_bytes());
        pfa[4..8].copy_from_slice(&top_wid.to_le_bytes());
        pfa[8..12].copy_from_slice(&gc.to_le_bytes());
        pfa[12..14].copy_from_slice(&80u16.to_le_bytes());
        pfa[14..16].copy_from_slice(&30u16.to_le_bytes());
        pfa[16..18].copy_from_slice(&40u16.to_le_bytes());
        pfa[18..20].copy_from_slice(&40u16.to_le_bytes());
        pfa[22..24].copy_from_slice(&(360i16 * 64).to_le_bytes());
        stream.write_all(&pfa).unwrap();

        let mut pt8 = [0u8; 20];
        pt8[0] = 74;
        pt8[2..4].copy_from_slice(&5u16.to_le_bytes());
        pt8[4..8].copy_from_slice(&child_wid.to_le_bytes());
        pt8[8..12].copy_from_slice(&gc.to_le_bytes());
        pt8[12..14].copy_from_slice(&14u16.to_le_bytes());
        pt8[14..16].copy_from_slice(&20u16.to_le_bytes());
        pt8[16] = 1; // len
        pt8[17] = 0; // delta
        pt8[18] = b'7';
        stream.write_all(&pt8).unwrap();

        for _ in 0..20 {
            desktop.poll_display_server();
            if desktop.windows.iter().any(|w| matches!(w.id, desktop::AppId::Guest(_))) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        let gid = desktop
            .windows
            .iter()
            .find_map(|w| if let desktop::AppId::Guest(g) = w.id { Some(g) } else { None })
            .expect("mapped guest window");
        let surf = desktop.display_srv.window(gid).unwrap();
        // Child button interior pixel (25, 25) should be gray 0x00C0C0C0, not white 0x00FFFFFF
        assert_eq!(surf.pixels[25 * 200 + 25], 0x00C0_C0C0);
        // Filled arc center (100, 50) should be GC foreground 0x00112233
        assert_eq!(surf.pixels[50 * 200 + 100], 0x0011_2233);

        // Both Expose (top + child) must be received
        let n = stream.read(&mut reply).unwrap();
        let exposes: Vec<u32> = reply[..n]
            .as_chunks::<32>()
            .0
            .iter()
            .filter(|e| e[0] == 12)
            .map(|e| u32::from_le_bytes([e[4], e[5], e[6], e[7]]))
            .collect();
        assert!(exposes.contains(&top_wid) && exposes.contains(&child_wid));

        // Clicking inside child button delivers ButtonPress targeting child_wid
        desktop.display_srv.send_pointer(gid, 30, 30, Some((1, true)));
        let n = stream.read(&mut reply).unwrap();
        let pressed_win = reply[..n]
            .as_chunks::<32>()
            .0
            .iter()
            .find(|e| e[0] == 4)
            .map(|e| u32::from_le_bytes([e[12], e[13], e[14], e[15]]));
        assert_eq!(pressed_win, Some(child_wid));
    }
}
