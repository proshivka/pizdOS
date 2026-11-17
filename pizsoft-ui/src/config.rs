use std::env;
use std::fs;
use std::path::PathBuf;

use crate::apps::{Lang, SettingsState, ThemePreset};

pub struct PersistedState {
    pub best_snake: u32,
    pub sticky_note: String,
}

impl Default for PersistedState {
    fn default() -> Self {
        Self {
            best_snake: 0,
            sticky_note: String::from("pizdOS 2.1(00)\n- No GNU, No RCE\n- Pure Rust DE"),
        }
    }
}

pub fn config_path() -> PathBuf {
    let home = env::var("USERPROFILE")
        .or_else(|_| env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| env::temp_dir());
    home.join(".pizsoft.conf")
}

pub fn load_config(settings: &mut SettingsState) -> PersistedState {
    if cfg!(test) {
        return PersistedState::default();
    }
    load_config_from(&config_path(), settings)
}

pub fn load_config_from(path: &std::path::Path, settings: &mut SettingsState) -> PersistedState {
    let mut persisted = PersistedState::default();
    let Ok(raw) = fs::read_to_string(path) else {
        return persisted;
    };
    for line in raw.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let v = v.trim();
        match k {
            "lang" => settings.lang = if v == "en" { Lang::En } else { Lang::Ru },
            "theme" => {
                settings.theme = match v {
                    "crimson" => ThemePreset::CrimsonSunset,
                    "matrix" => ThemePreset::MatrixGlow,
                    "ocean" => ThemePreset::OceanGlass,
                    _ => ThemePreset::AuroraPeaks,
                };
            }
            "accent_idx" => {
                if let Ok(idx) = v.parse::<usize>() {
                    settings.accent_idx = idx.min(6);
                }
            }
            "show_grid" => settings.show_grid = v == "1",
            "show_icons" => settings.show_icons = v != "0",
            "taskbar_top" => settings.taskbar_top = v == "1",
            "floating_dock" => settings.floating_dock = v == "1",
            "animations" => settings.animations = v != "0",
            "genie_minimize" => settings.genie_minimize = v != "0",
            "persistent_island" => settings.persistent_island = v == "1",
            "aero_stars" => settings.aero_stars = v != "0",
            "vista_sidebar" => settings.vista_sidebar = v != "0",
            "night_shift" => settings.night_shift = v == "1",
            "crt_scanlines" => settings.crt_scanlines = v == "1",
            "macos_buttons" => settings.macos_buttons = v == "1",
            "glass_level" => {
                if let Ok(g) = v.parse::<u8>() {
                    settings.glass_level = g.min(2);
                }
            }
            "ui_scale_pct" => {
                if let Ok(s) = v.parse::<u16>()
                    && matches!(s, 100 | 125 | 150 | 175)
                {
                    settings.ui_scale_pct = s;
                }
            }
            "res_idx" => {
                if let Ok(r) = v.parse::<usize>() {
                    settings.res_idx = r.min(crate::apps::SCREEN_RES_PRESETS.len() - 1);
                }
            }
            "show_clock" => settings.show_clock = v != "0",
            "best_snake" => {
                if let Ok(b) = v.parse::<u32>() {
                    persisted.best_snake = b;
                }
            }
            "sticky_note" => {
                persisted.sticky_note = v.replace("\\n", "\n");
            }
            "ai_api_key" => {
                settings.ai_api_key = v.to_string();
            }
            "ai_model_idx" => {
                if let Ok(m) = v.parse::<usize>() {
                    settings.ai_model_idx = m.min(crate::apps::AI_MODELS.len().saturating_sub(1));
                }
            }
            "ai_provider" => {
                if let Ok(p) = v.parse::<u8>() {
                    settings.ai_provider = p.min(2);
                }
            }
            "ai_base_url" => {
                settings.ai_base_url = v.to_string();
            }
            "ai_custom_model" => {
                settings.ai_custom_model = v.to_string();
            }
            "ai_thinking_idx" => {
                if let Ok(t) = v.parse::<usize>() {
                    settings.ai_thinking_idx = t.min(5);
                }
            }
            "ai_thinking_mask" => {
                if let Ok(mk) = v.parse::<u8>()
                    && mk != 0
                {
                    settings.ai_thinking_mask = mk & 0x3F;
                }
            }
            "ai_show_full_thinking" => {
                settings.ai_show_full_thinking = v == "1";
            }
            "ai_temp_idx" => {
                if let Ok(t) = v.parse::<u8>() {
                    settings.ai_temp_idx = t.min(2);
                }
            }
            "ai_system_prompt" => {
                settings.ai_system_prompt = v.replace("\\n", "\n");
            }
            _ => {}
        }
    }
    persisted
}

pub fn save_config(settings: &SettingsState, best_snake: u32, sticky_note: &str) {
    if cfg!(test) {
        return;
    }
    save_config_to(&config_path(), settings, best_snake, sticky_note);
}

pub fn save_config_to(
    path: &std::path::Path,
    settings: &SettingsState,
    best_snake: u32,
    sticky_note: &str,
) {
    let theme_str = match settings.theme {
        ThemePreset::AuroraPeaks => "aurora",
        ThemePreset::CrimsonSunset => "crimson",
        ThemePreset::MatrixGlow => "matrix",
        ThemePreset::OceanGlass => "ocean",
    };
    let lang_str = match settings.lang {
        Lang::Ru => "ru",
        Lang::En => "en",
    };
    let b = |v: bool| if v { "1" } else { "0" };
    let escaped_note = sticky_note.replace('\r', "").replace('\n', "\\n");
    let out = format!(
        "lang={lang_str}\n\
         theme={theme_str}\n\
         accent_idx={}\n\
         show_grid={}\n\
         show_icons={}\n\
         taskbar_top={}\n\
         floating_dock={}\n\
         animations={}\n\
         genie_minimize={}\n\
         persistent_island={}\n\
         aero_stars={}\n\
         vista_sidebar={}\n\
         night_shift={}\n\
         crt_scanlines={}\n\
         macos_buttons={}\n\
         glass_level={}\n\
         ui_scale_pct={}\n\
         res_idx={}\n\
         show_clock={}\n\
         best_snake={best_snake}\n\
         sticky_note={escaped_note}\n\
         ai_api_key={}\n\
         ai_model_idx={}\n\
         ai_provider={}\n\
         ai_base_url={}\n\
         ai_custom_model={}\n\
         ai_thinking_idx={}\n\
         ai_thinking_mask={}\n\
         ai_show_full_thinking={}\n\
         ai_temp_idx={}\n\
         ai_system_prompt={}\n",
        settings.accent_idx,
        b(settings.show_grid),
        b(settings.show_icons),
        b(settings.taskbar_top),
        b(settings.floating_dock),
        b(settings.animations),
        b(settings.genie_minimize),
        b(settings.persistent_island),
        b(settings.aero_stars),
        b(settings.vista_sidebar),
        b(settings.night_shift),
        b(settings.crt_scanlines),
        b(settings.macos_buttons),
        settings.glass_level,
        settings.ui_scale_pct,
        settings.res_idx,
        b(settings.show_clock),
        settings.ai_api_key.replace(['\r', '\n'], ""),
        settings.ai_model_idx.min(crate::apps::AI_MODELS.len().saturating_sub(1)),
        settings.ai_provider.min(2),
        settings.ai_base_url.replace(['\r', '\n'], ""),
        settings.ai_custom_model.replace(['\r', '\n'], ""),
        settings.ai_thinking_idx.min(5),
        settings.ai_thinking_mask & 0x3F,
        b(settings.ai_show_full_thinking),
        settings.ai_temp_idx.min(2),
        settings.ai_system_prompt.replace('\r', "").replace('\n', "\\n"),
    );
    let _ = fs::write(path, out);
}
