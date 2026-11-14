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
        // Click UI Scale 125% inside Control Center (idx 1 -> px = 1160 - 276 - 8 = 876, sx = 876 + 10 + 65 = 951, py = 720 - 36 - 266 - 6 = 412, scale_y = 412 + 170 = 582)
        desktop.on_mouse_down(965, 594, 1160, 720);
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

        // Toggle Always-On Dynamic Island back off inside Control Center (py = 412, island_y = 614..640)
        desktop.on_mouse_down(960, 624, 1160, 720);
        assert!(!desktop.settings.persistent_island);
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
}
