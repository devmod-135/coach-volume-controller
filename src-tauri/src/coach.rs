//! ETS2 coach (dlc_bus) sound volume - core logic.

use crate::fmod_bank::{parse_guids_file, Bank};
use crate::hashfs::HashFS;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

pub const GAME_FOLDER: &str = "Euro Truck Simulator 2";
pub const DLC_ARCHIVE: &str = "dlc_bus.scs";
/// Lookup order = override order (first hit wins): DLC first, then base game.
const BASE_ARCHIVES: [&str; 4] = ["def.scs", "base.scs", "base_share.scs", "core.scs"];
pub const SETTINGS_IN_MOD: &str = "coach_volume_settings.json";
const BUS_BANK_PREFIX: &str = "/sound/bus/";
pub const DEFAULT_MOD_NAME: &str = "Coach Sound Volume (custom)";

pub fn coach_name(id: &str) -> String {
    match id {
        "man.lions_24" => "MAN Lion's Coach",
        "scania.touring_23" => "Scania Touring",
        "volvo.9700_23" => "Volvo 9700",
        other => other,
    }
    .to_string()
}

// ---------------------------------------------------------------- language
static EN: AtomicBool = AtomicBool::new(false);

pub fn set_english(en: bool) {
    EN.store(en, Ordering::Relaxed);
}

pub fn en() -> bool {
    EN.load(Ordering::Relaxed)
}

/// tr!("한국어 {x}", "English {x}") -> String in the current language.
#[macro_export]
macro_rules! tr {
    ($ko:literal, $en:literal $(,)?) => {
        if $crate::coach::en() { format!($en) } else { format!($ko) }
    };
}

/// (slot id, Korean, English)
pub const SLOT_LABELS: &[(&str, &str, &str)] = &[
    ("air_warning", "공기압 경고음", "Air pressure warning"),
    ("blinker_on", "방향지시등 (켜짐)", "Turn signal (on)"),
    ("blinker_off", "방향지시등 (꺼짐)", "Turn signal (off)"),
    ("noise", "실내 주행 소음", "Interior driving noise"),
    ("stick_entrance_doors", "출입문 스위치", "Entrance door switch"),
    ("stick_blinker", "방향지시 레버", "Turn signal stalk"),
    ("stick_blinker_off", "방향지시 레버 (복귀)", "Turn signal stalk (return)"),
    ("stick_hazard_warning", "비상등 스위치", "Hazard light switch"),
    ("stick_high_beam", "상향등 레버", "High beam stalk"),
    ("stick_light_horn", "패싱 레버", "Headlight flasher stalk"),
    ("stick_lights", "전조등 스위치", "Headlight switch"),
    ("stick_park_brake", "주차 브레이크 (체결)", "Parking brake (engage)"),
    ("stick_park_brake_off", "주차 브레이크 (해제)", "Parking brake (release)"),
    ("stick_retarder", "리타더 레버", "Retarder stalk"),
    ("stick_wipers", "와이퍼 레버", "Wiper stalk"),
    ("weigh_green", "계근소 신호 (통과)", "Weigh station signal (pass)"),
    ("weigh_red", "계근소 신호 (정지)", "Weigh station signal (stop)"),
    ("window_click", "창문 스위치", "Window switch"),
    ("windowshade_move", "햇빛가리개", "Sun shade"),
    ("window_move", "창문 이동", "Window movement"),
    ("wipers_down", "와이퍼 (하강)", "Wipers (down)"),
    ("wipers_up", "와이퍼 (상승)", "Wipers (up)"),
    ("system_warning1", "시스템 경고음 1", "System warning 1"),
    ("system_warning2", "시스템 경고음 2", "System warning 2"),
    ("system_warning3", "시스템 경고음 3", "System warning 3"),
    ("bus_passengers", "승객 소음", "Passenger chatter"),
    ("entrance_doors_open", "출입문 열림", "Entrance doors opening"),
    ("entrance_doors_close", "출입문 닫힘", "Entrance doors closing"),
    ("entrance_doors_open_ext", "출입문 열림 (외부에서 들리는 소리)", "Entrance doors opening (heard outside)"),
    ("entrance_doors_close_ext", "출입문 닫힘 (외부에서 들리는 소리)", "Entrance doors closing (heard outside)"),
    ("engine", "엔진 (시동·공회전·주행음)", "Engine (start, idle, driving)"),
    ("exhaust", "배기음", "Exhaust"),
    ("start_bad", "시동 실패음", "Failed engine start"),
    ("turbo", "터보", "Turbo"),
    ("remote", "엔진 (원거리용)", "Engine (distant)"),
    ("retarder", "리타더", "Retarder"),
    ("retarder_remote", "리타더 (원거리용)", "Retarder (distant)"),
    ("air_gear", "변속 공기음", "Gear shift air"),
    ("gear_grind", "기어 갈리는 소리", "Gear grind"),
    ("gear_wrong", "잘못된 변속음", "Wrong gear"),
    ("air_brake", "에어브레이크 (치익)", "Air brake (hiss)"),
    ("air_cutoff", "에어 컷오프 (공기압 배출음)", "Air cut-off (pressure release)"),
    ("reverse", "후진 경고음", "Reverse beeper"),
    ("noise_suspension", "서스펜션 소음", "Suspension noise"),
    ("horn", "경적", "Horn"),
    ("air_horn", "에어혼", "Air horn"),
    ("aero_noise", "바람 소음", "Wind noise"),
    ("rain_noise", "빗소리", "Rain"),
];

pub fn slot_label(slot: &str) -> &str {
    let e = en();
    SLOT_LABELS.iter().find(|x| x.0 == slot).map_or(slot, |x| if e { x.2 } else { x.1 })
}

/// vehicle part folder -> category shown in the UI (in display order): (id, Korean, English)
pub const PART_CATEGORIES: [(&str, &str, &str); 5] = [
    ("interior", "실내 효과음", "Interior effects"),
    ("engine", "엔진", "Engine"),
    ("chassis", "브레이크·차체", "Brakes & chassis"),
    ("transmission", "변속기·리타더", "Transmission & retarder"),
    ("cabin", "경적·기타", "Horns & misc"),
];

fn re(cell: &'static OnceLock<Regex>, pat: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pat).unwrap())
}

// ---------------------------------------------------------------- paths
fn reg_string(hive: winreg::HKEY, key: &str, val: &str) -> Option<String> {
    winreg::RegKey::predef(hive).open_subkey(key).ok()?.get_value::<String, _>(val).ok()
}

fn expand_env(s: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"%([^%]+)%")
        .replace_all(s, |c: &regex::Captures| std::env::var(&c[1]).unwrap_or_else(|_| c[0].to_string()))
        .into_owned()
}

fn steam_paths() -> Vec<PathBuf> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let mut roots: Vec<PathBuf> = [
        reg_string(HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath"),
        reg_string(HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath"),
    ]
    .into_iter()
    .flatten()
    .map(|s| PathBuf::from(s.replace('/', "\\")))
    .collect();
    roots.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
    static R: OnceLock<Regex> = OnceLock::new();
    let path_re = re(&R, r#""path"\s+"([^"]+)""#);
    let mut libs = vec![];
    for r in roots {
        libs.push(r.clone());
        if let Ok(txt) = std::fs::read(r.join("steamapps").join("libraryfolders.vdf")) {
            for c in path_re.captures_iter(&String::from_utf8_lossy(&txt)) {
                libs.push(PathBuf::from(c[1].replace("\\\\", "\\")));
            }
        }
    }
    let mut seen = BTreeSet::new();
    libs.retain(|p| seen.insert(p.to_string_lossy().to_lowercase()));
    libs
}

pub fn find_game_dir() -> Option<String> {
    steam_paths().into_iter().find_map(|lib| {
        let p = lib.join("steamapps").join("common").join(GAME_FOLDER);
        p.join(DLC_ARCHIVE).is_file().then(|| p.to_string_lossy().into_owned())
    })
}

pub fn documents_dir() -> PathBuf {
    use winreg::enums::HKEY_CURRENT_USER;
    reg_string(HKEY_CURRENT_USER, r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders", "Personal")
        .map(|s| PathBuf::from(expand_env(&s)))
        .unwrap_or_else(|| PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Documents"))
}

pub fn default_mod_dir() -> String {
    documents_dir().join(GAME_FOLDER).join("mod").to_string_lossy().into_owned()
}

// ---------------------------------------------------------------- game data
pub struct SoundEvent {
    pub key: String,
    pub bank: String,
    pub event: String,
    pub slots: Vec<String>,
    pub coaches: Vec<String>,
    pub base_db: f64,
    pub category: String, // part id (see PART_CATEGORIES)
}

impl SoundEvent {
    pub fn label(&self) -> String {
        let mut labels: Vec<&str> = vec![];
        for s in &self.slots {
            let l = slot_label(s);
            if !labels.contains(&l) {
                labels.push(l);
            }
        }
        labels.join(" / ")
    }
    pub fn is_exterior(&self) -> bool {
        self.slots.iter().all(|s| s.ends_with("_ext"))
    }
}

pub struct SkippedSlot {
    pub coach: String,
    pub slot: String,
}

pub struct GameData {
    pub game_dir: String,
    archives: Vec<HashFS>,
    pub coaches: Vec<String>,
    pub events: BTreeMap<String, SoundEvent>,
    pub skipped: Vec<SkippedSlot>,
    banks: HashMap<String, Arc<Vec<u8>>>,
}

impl GameData {
    pub fn open(game_dir: &str) -> Result<Self, String> {
        let dir = Path::new(game_dir);
        if !dir.join(DLC_ARCHIVE).is_file() {
            return Err(tr!("{DLC_ARCHIVE} 가 없습니다. 코치 DLC가 설치돼 있는지 확인하세요.\n({game_dir})", "{DLC_ARCHIVE} not found. Make sure the coach DLC is installed.\n({game_dir})"));
        }
        let mut archives = vec![];
        for name in std::iter::once(DLC_ARCHIVE).chain(BASE_ARCHIVES) {
            let p = dir.join(name);
            if p.is_file() {
                archives.push(HashFS::open(&p.to_string_lossy())?);
            }
        }
        let mut g = Self {
            game_dir: game_dir.to_string(),
            archives,
            coaches: vec![],
            events: BTreeMap::new(),
            skipped: vec![],
            banks: HashMap::new(),
        };
        g.scan();
        Ok(g)
    }

    // layered virtual file system ------------------------------------
    fn exists(&self, path: &str) -> bool {
        self.archives.iter().any(|a| a.exists(path))
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        for a in &self.archives {
            if a.exists(path) {
                return a.read(path);
            }
        }
        Err(tr!("파일 없음: {path}", "File not found: {path}"))
    }

    fn read_text(&self, path: &str) -> Result<String, String> {
        let b = self.read(path)?;
        let s = String::from_utf8_lossy(&b);
        Ok(s.strip_prefix('\u{feff}').unwrap_or(&s).to_string())
    }

    fn listdir(&self, path: &str, files: bool) -> Vec<String> {
        let mut names: Vec<String> = vec![];
        for a in &self.archives {
            if a.is_dir(path) {
                if let Ok((d, f)) = a.listdir(path) {
                    for n in if files { f } else { d } {
                        if !names.contains(&n) {
                            names.push(n);
                        }
                    }
                }
            }
        }
        names
    }

    // scanning -------------------------------------------------------
    fn resolve(&self, r: &str) -> Option<String> {
        static R: OnceLock<Regex> = OnceLock::new();
        let r = r.trim();
        if r.is_empty() || r == "-" {
            return None;
        }
        if r.ends_with(".soundref") {
            let txt = self.read_text(r).ok()?;
            return re(&R, r#"source\s*=\s*"([^"]+)""#).captures(&txt).map(|c| c[1].to_string());
        }
        Some(r.to_string())
    }

    fn scan(&mut self) {
        let mut coaches = self.listdir("def/vehicle/bus", false);
        coaches.sort();
        for coach in coaches {
            if !self.exists(&format!("def/vehicle/bus/{coach}/interior/sound.sui")) {
                continue;
            }
            self.coaches.push(coach.clone());
            for (part, _, _) in PART_CATEGORIES {
                let folder = format!("def/vehicle/bus/{coach}/{part}");
                let mut names = self.listdir(&folder, true);
                names.sort();
                for name in names.iter().filter(|n| n.ends_with(".sui")) {
                    if let Ok(text) = self.read_text(&format!("{folder}/{name}")) {
                        self.scan_sui(&coach, part, &text);
                    }
                }
            }
        }
        self.read_base_levels();
    }

    fn scan_sui(&mut self, coach: &str, category: &str, text: &str) {
        static R: OnceLock<Regex> = OnceLock::new();
        let sounds = re(&R, r#"sounds\[\]\s*:\s*"([^|"]+)\|([^"]*)""#);
        for c in sounds.captures_iter(text) {
            let (slot, r) = (c[1].to_string(), c[2].to_string());
            let src = self.resolve(&r);
            let Some((bank, event)) = src.as_deref().and_then(|s| s.split_once('#')) else {
                self.skipped.push(SkippedSlot { coach: coach.into(), slot });
                continue;
            };
            if !bank.starts_with(BUS_BANK_PREFIX) {
                // shared with trucks: never touched
                self.skipped.push(SkippedSlot { coach: coach.into(), slot });
                continue;
            }
            let key = format!("{bank}#{event}");
            let ev = self.events.entry(key.clone()).or_insert_with(|| SoundEvent {
                key,
                bank: bank.into(),
                event: event.into(),
                slots: vec![],
                coaches: vec![],
                base_db: 0.0,
                category: category.into(),
            });
            if !ev.slots.contains(&slot) {
                ev.slots.push(slot);
            }
            if !ev.coaches.iter().any(|x| x == coach) {
                ev.coaches.push(coach.into());
            }
        }
    }

    fn read_base_levels(&mut self) {
        let banks: BTreeSet<String> = self.events.values().map(|e| e.bank.clone()).collect();
        let mut drop = vec![];
        for bank_path in banks {
            let bank = self.load_bank(&bank_path).ok();
            for ev in self.events.values_mut().filter(|e| e.bank == bank_path) {
                match bank.as_ref().and_then(|b| b.event(&format!("event:/{}", ev.event))) {
                    Some(be) => ev.base_db = be.total_db(),
                    None => drop.push(ev.key.clone()),
                }
            }
        }
        for k in drop {
            if let Some(ev) = self.events.remove(&k) {
                self.skipped.push(SkippedSlot { coach: ev.coaches.join(","), slot: ev.slots.join(",") });
            }
        }
    }

    pub fn bank_bytes(&mut self, bank_path: &str) -> Result<Arc<Vec<u8>>, String> {
        if let Some(b) = self.banks.get(bank_path) {
            return Ok(b.clone());
        }
        let b = Arc::new(self.read(bank_path)?);
        self.banks.insert(bank_path.to_string(), b.clone());
        Ok(b)
    }

    fn bank_names(&self, bank_path: &str) -> HashMap<String, String> {
        self.read_text(&format!("{bank_path}.guids")).map(|t| parse_guids_file(&t)).unwrap_or_default()
    }

    pub fn load_bank(&mut self, bank_path: &str) -> Result<Bank, String> {
        let bytes = self.bank_bytes(bank_path)?;
        Bank::from_bytes(&bytes, &self.bank_names(bank_path))
    }

    pub fn bank_sha1(&mut self, bank_path: &str) -> Result<String, String> {
        let b = self.bank_bytes(bank_path)?;
        Ok(Sha1::digest(&b[..]).iter().map(|x| format!("{x:02x}")).collect())
    }
}

// ---------------------------------------------------------------- settings
fn one() -> u32 {
    1
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Settings {
    #[serde(default = "one")]
    pub version: u32,
    #[serde(default)]
    pub global_db: f64,
    #[serde(default)]
    pub per_event: BTreeMap<String, f64>,
    #[serde(default)]
    pub source_sha1: BTreeMap<String, String>,
    /// Display name of the mod in the game's mod manager (only stored inside mods).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mod_name: Option<String>,
}

impl Settings {
    pub fn gain_for(&self, key: &str) -> f64 {
        self.global_db + self.per_event.get(key).copied().unwrap_or(0.0)
    }
}

fn read_zip_entry(path: &str, name: &str) -> Option<String> {
    let f = std::fs::File::open(path).ok()?;
    let mut z = zip::ZipArchive::new(f).ok()?;
    let mut e = z.by_name(name).ok()?;
    let mut s = String::new();
    e.read_to_string(&mut s).ok()?;
    Some(s)
}

pub fn load_settings_from_mod(path: &str) -> Option<Settings> {
    let mut s: Settings = serde_json::from_str(&read_zip_entry(path, SETTINGS_IN_MOD)?).ok()?;
    if s.mod_name.is_none() {
        // mods made by the old (Python) version: take the name from the manifest
        static R: OnceLock<Regex> = OnceLock::new();
        let manifest = read_zip_entry(path, "manifest.sii").unwrap_or_default();
        s.mod_name = re(&R, r#"display_name\s*:\s*"([^"]*)""#)
            .captures(&manifest)
            .map(|c| c[1].to_string())
            .or_else(|| Some(DEFAULT_MOD_NAME.into()));
    }
    Some(s)
}

/// A mod made by this tool (it carries our settings file).
#[derive(Serialize, Clone)]
pub struct ModInfo {
    pub name: String,
    pub path: String,
    pub file_name: String,
    pub modified: u64,
}

pub fn list_mods(dir: &str) -> Vec<ModInfo> {
    let mut out = vec![];
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if !p.extension().map_or(false, |x| x.eq_ignore_ascii_case("scs")) {
            continue;
        }
        let path = p.to_string_lossy().into_owned();
        if let Some(s) = load_settings_from_mod(&path) {
            let modified = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            out.push(ModInfo {
                name: s.mod_name.unwrap_or_default(),
                file_name: e.file_name().to_string_lossy().into_owned(),
                path,
                modified,
            });
        }
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    out
}

pub fn is_tool_mod(path: &str) -> bool {
    read_zip_entry(path, SETTINGS_IN_MOD).is_some()
}

// ---------------------------------------------------------------- presets
pub const LAST_SESSION: &str = "(마지막 작업)";
const LAST_FILE: &str = "_last_session.json";

pub fn sanitize_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || (c as u32) < 0x20 { '_' } else { c })
        .collect();
    s.trim_matches(|c| c == ' ' || c == '.').to_string()
}

pub struct Presets {
    pub dir: PathBuf,
}

impl Presets {
    fn file(&self, name: &str) -> Result<PathBuf, String> {
        if name == LAST_SESSION {
            return Ok(self.dir.join(LAST_FILE));
        }
        let safe = sanitize_name(name);
        if safe.is_empty() {
            return Err(tr!("프리셋 이름이 비어 있습니다.", "The preset name is empty."));
        }
        Ok(self.dir.join(format!("{safe}.json")))
    }

    /// Preset names, newest first. The auto-saved last session (if any) comes first.
    pub fn list(&self) -> Vec<String> {
        let _ = std::fs::create_dir_all(&self.dir);
        let mut files: Vec<(std::time::SystemTime, String)> = vec![];
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                let Some(stem) = n.strip_suffix(".json") else { continue };
                if n == LAST_FILE {
                    continue;
                }
                if let Ok(t) = e.metadata().and_then(|m| m.modified()) {
                    files.push((t, stem.to_string()));
                }
            }
        }
        files.sort_by(|a, b| b.0.cmp(&a.0));
        let mut names: Vec<String> = files.into_iter().map(|f| f.1).collect();
        if self.dir.join(LAST_FILE).is_file() {
            names.insert(0, LAST_SESSION.into());
        }
        names
    }

    pub fn save(&self, name: &str, s: &Settings) -> Result<String, String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let path = self.file(name)?;
        let clean = Settings { global_db: s.global_db, per_event: s.per_event.clone(), ..Default::default() };
        let json = serde_json::to_string_pretty(&clean).map_err(|e| e.to_string())?;
        write_atomic(&path, json.as_bytes())?;
        Ok(path.to_string_lossy().into_owned())
    }

    pub fn load(&self, name: &str) -> Result<Settings, String> {
        let txt = std::fs::read_to_string(self.file(name)?).map_err(|e| e.to_string())?;
        let mut s: Settings = serde_json::from_str(&txt).map_err(|e| e.to_string())?;
        s.source_sha1.clear();
        s.mod_name = None;
        Ok(s)
    }

    pub fn delete(&self, name: &str) -> Result<(), String> {
        std::fs::remove_file(self.file(name)?).map_err(|e| e.to_string())
    }
}

/// Load settings from a mod (.scs) or a preset/settings .json file.
pub fn load_settings_file(path: &str) -> Option<Settings> {
    let mut s = if path.to_lowercase().ends_with(".json") {
        let mut s: Settings = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
        s.mod_name = None;
        s
    } else {
        load_settings_from_mod(path)?
    };
    s.source_sha1.clear();
    Some(s)
}

/// Banks whose original changed (game update) since the mod was built.
pub fn stale_banks(game: &mut GameData, s: &Settings) -> Vec<String> {
    s.source_sha1
        .iter()
        .filter(|(bank, sha)| game.bank_sha1(bank).map_or(true, |h| &h != *sha))
        .map(|(b, _)| b.clone())
        .collect()
}

pub fn io_err(e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        tr!("파일을 쓸 수 없습니다. 게임이 실행 중이면 종료한 뒤 다시 시도하세요.", "Cannot write the file. If the game is running, close it and try again.")
    } else {
        e.to_string()
    }
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    let tmp = PathBuf::from(format!("{}.tmp", path.to_string_lossy()));
    std::fs::write(&tmp, data).map_err(io_err)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io_err(e)
    })
}

// ---------------------------------------------------------------- build
fn manifest(display_name: &str) -> String {
    let name: String = display_name.chars().filter(|c| *c != '"' && *c != '\\' && !c.is_control()).collect();
    format!(
        "SiiNunit\n{{\nmod_package : .coach_interior_volume {{\n\tpackage_version: \"1.0\"\n\tdisplay_name: \"{name}\"\n\tauthor: \"ETS2 Coach Volume Controller\"\n\tcategory[]: \"sound\"\n\tdescription_file: \"description.txt\"\n}}\n}}\n"
    )
}

#[derive(Serialize)]
pub struct Clamp {
    pub key: String,
    pub wanted: f64,
    pub applied: f64,
}

#[derive(Serialize)]
pub struct BuildResult {
    pub path: Option<String>,
    pub banks: Vec<String>,
    pub changed_events: usize,
    pub clamped: Vec<Clamp>,
    pub settings: Settings,
}

pub fn build_mod(game: &mut GameData, mut settings: Settings, mod_name: &str, out_path: &str) -> Result<BuildResult, String> {
    let mut by_bank: BTreeMap<String, Vec<(String, String, String)>> = BTreeMap::new(); // bank -> (key, event, label)
    for ev in game.events.values() {
        if settings.gain_for(&ev.key).abs() >= 0.005 {
            by_bank.entry(ev.bank.clone()).or_default().push((ev.key.clone(), ev.event.clone(), ev.label()));
        }
    }
    settings.mod_name = Some(mod_name.to_string());
    settings.source_sha1.clear();

    if by_bank.is_empty() {
        // nothing to change -> no mod needed
        if Path::new(out_path).exists() {
            std::fs::remove_file(out_path).map_err(|e| e.to_string())?;
        }
        return Ok(BuildResult { path: None, banks: vec![], changed_events: 0, clamped: vec![], settings });
    }

    let mut clamped = vec![];
    let mut files: Vec<(String, Vec<u8>)> = vec![];
    let mut lines = vec![];
    let mut changed = 0;
    for (bank_path, evs) in &by_bank {
        let mut bank = game.load_bank(bank_path)?;
        for (key, event, label) in evs {
            let want = settings.gain_for(key);
            let got = bank
                .apply_gain(&format!("event:/{event}"), want)
                .ok_or_else(|| tr!("뱅크에서 이벤트를 찾지 못함: {event}", "Event not found in bank: {event}"))?;
            if (got - want).abs() > 0.01 {
                clamped.push(Clamp { key: key.clone(), wanted: want, applied: got });
            }
            lines.push(format!("{want:+.1} dB  {label}  ({event})"));
            changed += 1;
        }
        files.push((bank_path.trim_start_matches('/').to_string(), bank.data));
        let sha = game.bank_sha1(bank_path)?;
        settings.source_sha1.insert(bank_path.clone(), sha);
    }

    let body = lines.join("\n");
    let desc = tr!(
        "{mod_name}\n코치 DLC 사운드 볼륨 조절 모드 (ETS2 Coach Volume Controller로 생성)\n게임 업데이트 후에는 툴에서 [모드 만들기]를 다시 눌러주세요.\n\n{body}\n",
        "{mod_name}\nCoach DLC sound volume mod (made with ETS2 Coach Volume Controller)\nAfter a game update, press [Build mod] in the tool again.\n\n{body}\n"
    );

    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
        let head: [(&str, Vec<u8>); 3] = [
            ("manifest.sii", manifest(mod_name).into_bytes()),
            ("description.txt", desc.into_bytes()),
            (SETTINGS_IN_MOD, json.into_bytes()),
        ];
        let entries = head
            .iter()
            .map(|(n, d)| (*n, d.as_slice()))
            .chain(files.iter().map(|(n, d)| (n.as_str(), d.as_slice())));
        for (name, data) in entries {
            z.start_file(name, opt).map_err(|e| e.to_string())?;
            z.write_all(data).map_err(|e| e.to_string())?;
        }
        z.finish().map_err(|e| e.to_string())?;
    }
    if let Some(parent) = Path::new(out_path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    write_atomic(Path::new(out_path), buf.get_ref())?;
    Ok(BuildResult {
        path: Some(out_path.to_string()),
        banks: by_bank.keys().cloned().collect(),
        changed_events: changed,
        clamped,
        settings,
    })
}
