#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod coach;
mod fmod_bank;
mod hashfs;

use coach::{GameData, ModInfo, Presets, Settings};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{Manager, State};

struct AppState {
    game: Mutex<Option<GameData>>,
    presets: Presets,
    config_path: PathBuf,
}

type R<T> = Result<T, String>;

// ---------------------------------------------------------------- config
#[derive(Serialize, Deserialize, Default)]
struct Config {
    #[serde(default)]
    game_dir: Option<String>,
    #[serde(default)]
    mod_dir: Option<String>,
    #[serde(default)]
    mod_path: Option<String>,
    #[serde(default)]
    lang: Option<String>,
}

fn read_config(path: &Path) -> Config {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

#[derive(Serialize)]
struct Init {
    game_dir: Option<String>,
    mod_dir: String,
    mod_path: Option<String>,
    default_mod_name: String,
    last_session: String,
    lang: Option<String>,
}

#[tauri::command]
fn init(st: State<AppState>) -> Init {
    let cfg = read_config(&st.config_path);
    if let Some(l) = &cfg.lang {
        coach::set_english(l == "en");
    }
    let game_dir = cfg
        .game_dir
        .filter(|d| Path::new(d).join(coach::DLC_ARCHIVE).is_file())
        .or_else(coach::find_game_dir);
    Init {
        game_dir,
        mod_dir: cfg.mod_dir.filter(|d| Path::new(d).is_dir()).unwrap_or_else(coach::default_mod_dir),
        mod_path: cfg.mod_path,
        default_mod_name: coach::DEFAULT_MOD_NAME.into(),
        last_session: coach::LAST_SESSION.into(),
        lang: cfg.lang,
    }
}

#[tauri::command]
fn save_config(st: State<AppState>, game_dir: Option<String>, mod_dir: Option<String>, mod_path: Option<String>) -> R<()> {
    let lang = read_config(&st.config_path).lang;
    let cfg = Config { game_dir, mod_dir, mod_path, lang };
    if let Some(p) = st.config_path.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    std::fs::write(&st.config_path, serde_json::to_string_pretty(&cfg).unwrap()).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_lang(st: State<AppState>, lang: String) -> R<()> {
    coach::set_english(lang == "en");
    let mut cfg = read_config(&st.config_path);
    cfg.lang = Some(lang);
    if let Some(p) = st.config_path.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    std::fs::write(&st.config_path, serde_json::to_string_pretty(&cfg).unwrap()).map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct LangLabels {
    slots: BTreeMap<String, String>,
    parts: BTreeMap<String, String>,
}

/// Sound slot and category names for both languages: {"ko": {...}, "en": {...}}
#[tauri::command]
fn labels() -> BTreeMap<String, LangLabels> {
    let mk = |en: bool| LangLabels {
        slots: coach::SLOT_LABELS.iter().map(|(id, ko, e)| (id.to_string(), if en { *e } else { *ko }.to_string())).collect(),
        parts: coach::PART_CATEGORIES.iter().map(|(id, ko, e)| (id.to_string(), if en { *e } else { *ko }.to_string())).collect(),
    };
    BTreeMap::from([("ko".to_string(), mk(false)), ("en".to_string(), mk(true))])
}

// ---------------------------------------------------------------- game
#[derive(Serialize)]
struct CoachDto {
    id: String,
    name: String,
}

#[derive(Serialize)]
struct EventDto {
    key: String,
    bank: String,
    event: String,
    slots: Vec<String>,
    coaches: Vec<String>,
    base_db: f64,
    part: String,
    is_exterior: bool,
}

#[derive(Serialize)]
struct GameInfo {
    game_dir: String,
    coaches: Vec<CoachDto>,
    parts: Vec<String>,
    events: Vec<EventDto>,
    skipped: Vec<SkippedDto>,
}

#[derive(Serialize)]
struct SkippedDto {
    slots: Vec<String>,
    coaches: Vec<String>,
    all_coaches: bool,
}

#[tauri::command]
async fn load_game(st: State<'_, AppState>, path: String) -> R<GameInfo> {
    let mut slot = st.game.lock().unwrap();
    *slot = None;
    let g = GameData::open(&path)?;
    let mut by_slot: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for sk in &g.skipped {
        let v = by_slot.entry(sk.slot.clone()).or_default();
        if !v.contains(&sk.coach) {
            v.push(sk.coach.clone());
        }
    }
    let skipped = by_slot
        .into_iter()
        .map(|(s, cs)| SkippedDto {
            slots: s.split(',').map(String::from).collect(),
            all_coaches: cs.len() >= g.coaches.len(),
            coaches: cs.iter().flat_map(|c| c.split(',')).map(coach::coach_name).collect(),
        })
        .collect();
    let info = GameInfo {
        game_dir: g.game_dir.clone(),
        coaches: g.coaches.iter().map(|c| CoachDto { id: c.clone(), name: coach::coach_name(c) }).collect(),
        parts: coach::PART_CATEGORIES.iter().map(|(id, _, _)| id.to_string()).collect(),
        events: g
            .events
            .values()
            .map(|e| EventDto {
                key: e.key.clone(),
                bank: e.bank.clone(),
                event: e.event.clone(),
                slots: e.slots.clone(),
                coaches: e.coaches.clone(),
                base_db: e.base_db,
                part: e.category.clone(),
                is_exterior: e.is_exterior(),
            })
            .collect(),
        skipped,
    };
    *slot = Some(g);
    Ok(info)
}

#[tauri::command]
fn find_game() -> Option<String> {
    coach::find_game_dir()
}

#[tauri::command]
async fn stale_banks(st: State<'_, AppState>, settings: Settings) -> R<Vec<String>> {
    let mut g = st.game.lock().unwrap();
    let g = g.as_mut().ok_or_else(|| tr!("게임 데이터가 없습니다", "No game data loaded"))?;
    Ok(coach::stale_banks(g, &settings))
}

// ---------------------------------------------------------------- mods
#[tauri::command]
fn list_mods(dir: String) -> Vec<ModInfo> {
    coach::list_mods(&dir)
}

#[tauri::command]
fn read_mod_settings(path: String) -> Option<Settings> {
    coach::load_settings_from_mod(&path)
}

#[derive(Serialize)]
struct FileState {
    exists: bool,
    tool_mod: bool,
}

#[tauri::command]
fn file_state(path: String) -> FileState {
    let exists = Path::new(&path).exists();
    FileState { exists, tool_mod: exists && coach::is_tool_mod(&path) }
}

#[tauri::command]
fn mod_file_path(dir: String, name: String) -> R<String> {
    let safe = coach::sanitize_name(&name);
    if safe.is_empty() {
        return Err(tr!("모드 이름을 입력하세요.", "Enter a mod name."));
    }
    Ok(Path::new(&dir).join(format!("{safe}.scs")).to_string_lossy().into_owned())
}

#[tauri::command]
async fn build_mod(
    st: State<'_, AppState>,
    settings: Settings,
    mod_name: String,
    out_path: String,
    remove_old: Option<String>,
) -> R<coach::BuildResult> {
    let name = mod_name.trim();
    if name.is_empty() {
        return Err(tr!("모드 이름을 입력하세요.", "Enter a mod name."));
    }
    if Path::new(&out_path).exists() && !coach::is_tool_mod(&out_path) {
        return Err(tr!("이 툴로 만들지 않은 모드 파일이 이미 있어 덮어쓰지 않습니다:\n{out_path}", "A mod file not made by this tool already exists, so it was not overwritten:\n{out_path}"));
    }
    let res = {
        let mut g = st.game.lock().unwrap();
        let g = g.as_mut().ok_or_else(|| tr!("게임 데이터가 없습니다", "No game data loaded"))?;
        coach::build_mod(g, settings, name, &out_path)?
    };
    if let Some(old) = remove_old {
        if !old.eq_ignore_ascii_case(&out_path) && coach::is_tool_mod(&old) {
            std::fs::remove_file(&old).map_err(coach::io_err)?;
        }
    }
    Ok(res)
}

#[tauri::command]
fn delete_mod(path: String) -> R<()> {
    if !coach::is_tool_mod(&path) {
        return Err(tr!("이 툴로 만든 모드만 삭제할 수 있습니다.", "Only mods made by this tool can be deleted."));
    }
    std::fs::remove_file(&path).map_err(coach::io_err)
}

// ---------------------------------------------------------------- presets
#[tauri::command]
fn list_presets(st: State<AppState>) -> Vec<String> {
    st.presets.list()
}

#[tauri::command]
fn save_preset(st: State<AppState>, name: String, settings: Settings) -> R<String> {
    st.presets.save(name.trim(), &settings)
}

#[tauri::command]
fn load_preset(st: State<AppState>, name: String) -> R<Settings> {
    st.presets.load(&name)
}

#[tauri::command]
fn delete_preset(st: State<AppState>, name: String) -> R<()> {
    st.presets.delete(&name)
}

#[tauri::command]
fn load_settings_file(path: String) -> Option<Settings> {
    coach::load_settings_file(&path)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data = app.path().app_data_dir()?;
            app.manage(AppState {
                game: Mutex::new(None),
                presets: Presets { dir: data.join("presets") },
                config_path: data.join("config.json"),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            init,
            save_config,
            set_lang,
            labels,
            load_game,
            find_game,
            stale_banks,
            list_mods,
            read_mod_settings,
            file_state,
            mod_file_path,
            build_mod,
            delete_mod,
            list_presets,
            save_preset,
            load_preset,
            delete_preset,
            load_settings_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
