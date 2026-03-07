use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    pub device: DeviceConfig,
    pub capture: CaptureConfig,
    pub sync: SyncConfig,
    #[serde(default)]
    pub effect: EffectConfig,
    #[serde(default)]
    pub app: AppConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DeviceConfig {
    pub com_port: String,
    pub wire_map: String,
    pub display_size: u32,
    pub lamps_amount: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CaptureConfig {
    pub fps: u32,
    pub monitor: u32,
    pub sample_width: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SyncConfig {
    pub speed: u32,
    pub brightness: u8,
    #[serde(default = "default_gamma")]
    pub gamma: f32,
    #[serde(default = "default_saturation")]
    pub saturation: f32,
    #[serde(default = "default_true")]
    pub light_compression: bool,
    #[serde(default = "default_true")]
    pub smoothing: bool,
    #[serde(default)]
    pub reverse: bool,
    #[serde(default = "default_edge_number")]
    pub edge_number: u8,
}

/// LED 동작 모드
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LedMode {
    Sync,
    Dynamic,
    Sound,
    Static,
    Off,
}

impl Default for LedMode {
    fn default() -> Self { Self::Sync }
}

/// 음악 반응 소스
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RhythmSource {
    Controller,
    Computer,
}

impl Default for RhythmSource {
    fn default() -> Self { Self::Controller }
}

/// UI 언어
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Auto,
    En,
    Ko,
}

impl Default for Language {
    fn default() -> Self { Self::Auto }
}

/// 소프트웨어 동적 효과 (단색)
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SoftEffect {
    None,
    Breathe,
    Rotate,
}

impl Default for SoftEffect {
    fn default() -> Self { Self::None }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct EffectConfig {
    #[serde(default)]
    pub mode: LedMode,
    #[serde(default)]
    pub dynamic_index: u8,
    #[serde(default)]
    pub sound_index: u8,
    #[serde(default)]
    pub color_r: u8,
    #[serde(default)]
    pub color_g: u8,
    #[serde(default = "default_255")]
    pub color_b: u8,
    #[serde(default = "default_50")]
    pub effect_speed: u8,
    #[serde(default)]
    pub rhythm_source: RhythmSource,
    #[serde(default)]
    pub soft_effect: SoftEffect,
}

impl Default for EffectConfig {
    fn default() -> Self {
        Self {
            mode: LedMode::Sync,
            dynamic_index: 0,
            sound_index: 0,
            color_r: 0,
            color_g: 0,
            color_b: 255,
            effect_speed: 50,
            rhythm_source: RhythmSource::Controller,
            soft_effect: SoftEffect::None,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AppConfig {
    #[serde(default)]
    pub autostart: bool,
    #[serde(default)]
    pub show_console: bool,
    #[serde(default)]
    pub language: Language,
    #[serde(default)]
    pub turn_off_on_sleep: bool,
    #[serde(default)]
    pub turn_off_on_black: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            autostart: false,
            show_console: false,
            language: Language::Auto,
            turn_off_on_sleep: false,
            turn_off_on_black: false,
        }
    }
}

fn default_gamma() -> f32 { 1.0 }
fn default_saturation() -> f32 { 1.0 }
fn default_true() -> bool { true }
fn default_edge_number() -> u8 { 3 }
fn default_255() -> u8 { 255 }
fn default_50() -> u8 { 50 }

impl SyncConfig {
    pub fn interval_ms(&self) -> u64 {
        self.speed as u64 + 20
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn load_or_default() -> Self {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));

        let candidates = [
            exe_dir.as_ref().map(|d| d.join("config.toml")),
            Some(Path::new("config.toml").to_path_buf()),
        ];

        for candidate in candidates.iter().flatten() {
            if candidate.exists() {
                match Self::load(candidate) {
                    Ok(cfg) => {
                        log::info!("설정 로드 완료: {}", candidate.display());
                        return cfg;
                    }
                    Err(e) => {
                        log::warn!("설정 파일 파싱 실패 ({}): {}", candidate.display(), e);
                    }
                }
            }
        }

        log::info!("config.toml 없음, 기본 설정 사용");
        Self::default()
    }

    pub fn config_path() -> PathBuf {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));

        if let Some(ref dir) = exe_dir {
            let path = dir.join("config.toml");
            if path.exists() {
                return path;
            }
        }

        let cwd_path = PathBuf::from("config.toml");
        if cwd_path.exists() {
            return cwd_path;
        }

        exe_dir
            .map(|d| d.join("config.toml"))
            .unwrap_or_else(|| PathBuf::from("config.toml"))
    }

    pub fn save(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let header = "# SyncRGB 설정 파일\n\n";
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, format!("{}{}", header, content))?;
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            device: DeviceConfig {
                com_port: "auto".into(),
                wire_map: "RGB".into(),
                display_size: 27,
                lamps_amount: 65,
            },
            capture: CaptureConfig {
                fps: 30,
                monitor: 0,
                sample_width: 50,
            },
            sync: SyncConfig {
                speed: 0,
                brightness: 255,
                gamma: 1.0,
                saturation: 1.0,
                light_compression: true,
                smoothing: true,
                reverse: false,
                edge_number: 3,
            },
            effect: EffectConfig::default(),
            app: AppConfig::default(),
        }
    }
}

// ── 자동 시작 (레지스트리) ──

const REG_RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const REG_APPROVED_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const REG_VALUE_NAME: &str = "SyncRGB";

pub fn set_autostart(enable: bool) -> Result<(), Box<dyn std::error::Error>> {
    use winreg::enums::*;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    if enable {
        let exe = std::env::current_exe()?;
        // Run 키에 등록
        let run_key = hkcu.open_subkey_with_flags(REG_RUN_KEY, KEY_WRITE)?;
        run_key.set_value(REG_VALUE_NAME, &exe.to_string_lossy().as_ref())?;
        // StartupApproved에 활성화 상태로 등록 (0x02 = enabled)
        if let Ok(approved_key) = hkcu.open_subkey_with_flags(REG_APPROVED_KEY, KEY_WRITE) {
            let enabled = winreg::RegValue {
                vtype: RegType::REG_BINARY,
                bytes: vec![0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            };
            approved_key.set_raw_value(REG_VALUE_NAME, &enabled).ok();
        }
        log::info!("자동 시작 등록 완료 (레지스트리 + StartupApproved)");
    } else {
        if let Ok(run_key) = hkcu.open_subkey_with_flags(REG_RUN_KEY, KEY_WRITE) {
            run_key.delete_value(REG_VALUE_NAME).ok();
        }
        if let Ok(approved_key) = hkcu.open_subkey_with_flags(REG_APPROVED_KEY, KEY_WRITE) {
            approved_key.delete_value(REG_VALUE_NAME).ok();
        }
        log::info!("자동 시작 해제 완료");
    }

    // 기존 Task Scheduler 항목 정리
    cleanup_legacy_task();

    Ok(())
}

pub fn is_autostart_enabled() -> bool {
    use winreg::enums::*;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let has_run = hkcu.open_subkey(REG_RUN_KEY)
        .and_then(|key| key.get_value::<String, _>(REG_VALUE_NAME))
        .is_ok();
    if !has_run { return false; }

    // StartupApproved에서 비활성화(0x01) 여부 확인
    if let Ok(approved) = hkcu.open_subkey(REG_APPROVED_KEY) {
        if let Ok(val) = approved.get_raw_value(REG_VALUE_NAME) {
            if !val.bytes.is_empty() && val.bytes[0] & 0x01 != 0 && val.bytes[0] & 0x02 == 0 {
                return false; // 사용자가 작업 관리자에서 비활성화
            }
        }
    }
    true
}

/// 기존 Task Scheduler 항목 정리 (마이그레이션)
fn cleanup_legacy_task() {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("schtasks")
        .args(["/delete", "/tn", "SyncRGB", "/f"])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output();
}
