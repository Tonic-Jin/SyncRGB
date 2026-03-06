use eframe::egui;
use std::path::PathBuf;

use crate::config::{self, Config, LedMode, RhythmSource, SoftEffect};

const WIRE_MAPS: &[&str] = &["RGB", "RBG", "GRB", "GBR", "BRG", "BGR"];

// ── 다국어 (Locale) ──

pub struct Locale {
    pub tab_sync: &'static str,
    pub tab_dynamic: &'static str,
    pub tab_sound: &'static str,
    pub tab_static: &'static str,
    pub led_on: &'static str,
    pub led_off: &'static str,
    pub led_off_msg: &'static str,
    pub led_off_hint: &'static str,
    pub brightness: &'static str,
    pub settings: &'static str,
    pub applied: &'static str,
    pub save_failed_prefix: &'static str,
    pub failed_keyword: &'static str,
    pub sync_speed: &'static str,
    pub speed_fastest: &'static str,
    pub speed_fast: &'static str,
    pub speed_normal: &'static str,
    pub speed_slow: &'static str,
    pub color_processing: &'static str,
    pub saturation: &'static str,
    pub gamma: &'static str,
    pub light_compression: &'static str,
    pub smoothing: &'static str,
    pub led_layout: &'static str,
    pub direction: &'static str,
    pub dir_ltr: &'static str,
    pub dir_rtl: &'static str,
    pub edges: &'static str,
    pub edges_3: &'static str,
    pub edges_4: &'static str,
    pub pattern_hint: &'static str,
    pub effect_speed: &'static str,
    pub sound_source: &'static str,
    pub source_controller: &'static str,
    pub source_computer: &'static str,
    pub color_adjust: &'static str,
    pub quick_select: &'static str,
    pub static_animation: &'static str,
    pub anim_none: &'static str,
    pub anim_breathe: &'static str,
    pub anim_rotate: &'static str,
    pub speed: &'static str,
    pub device: &'static str,
    pub led_count: &'static str,
    pub monitor_label: &'static str,
    pub capture_fps: &'static str,
    pub app_section: &'static str,
    pub autostart: &'static str,
    pub autostart_failed_prefix: &'static str,
    pub save_settings: &'static str,
    pub dynamic_effects: &'static [&'static str],
    pub sound_effects: &'static [&'static str],
    pub color_presets: &'static [(&'static str, [u8; 3])],
    pub tray_pause: &'static str,
    pub tray_resume: &'static str,
    pub tray_settings: &'static str,
    pub tray_quit: &'static str,
    pub tray_paused_tooltip: &'static str,
    pub language_label: &'static str,
    pub lang_auto: &'static str,
}

static KO: Locale = Locale {
    tab_sync: "화면 동기화",
    tab_dynamic: "동적 효과",
    tab_sound: "음악 반응",
    tab_static: "단색",
    led_on: "LED ON",
    led_off: "LED OFF",
    led_off_msg: "LED가 꺼져 있습니다",
    led_off_hint: "모드 탭을 클릭하면 LED가 자동으로 켜집니다",
    brightness: "밝기:",
    settings: "설정",
    applied: "적용됨",
    save_failed_prefix: "저장 실패: ",
    failed_keyword: "실패",
    sync_speed: "동기화 속도",
    speed_fastest: "극빠름",
    speed_fast: "빠름",
    speed_normal: "보통",
    speed_slow: "느림",
    color_processing: "색상 처리",
    saturation: "채도",
    gamma: "감마",
    light_compression: "광량 압축 (USB 전력 최적화)",
    smoothing: "부드러운 전환 (스무딩)",
    led_layout: "LED 배치",
    direction: "방향:",
    dir_ltr: "왼쪽 → 오른쪽",
    dir_rtl: "오른쪽 → 왼쪽",
    edges: "변 수:",
    edges_3: "3면 (상/좌/우)",
    edges_4: "4면",
    pattern_hint: "패턴을 선택하면 즉시 적용됩니다",
    effect_speed: "효과 속도",
    sound_source: "음악 소스",
    source_controller: "컨트롤러 (내장 마이크)",
    source_computer: "컴퓨터 (PC 오디오)",
    color_adjust: "색상 조절",
    quick_select: "빠른 선택",
    static_animation: "단색 애니메이션",
    anim_none: "없음",
    anim_breathe: "숨쉬기",
    anim_rotate: "회전",
    speed: "속도",
    device: "디바이스",
    led_count: "LED 개수:",
    monitor_label: "모니터:",
    capture_fps: "캡처 FPS:",
    app_section: "앱",
    autostart: "Windows 시작 시 자동 실행",
    autostart_failed_prefix: "자동 시작 실패: ",
    save_settings: "  설정 저장  ",
    dynamic_effects: &[
        "무지개 흐름", "숨쉬기", "컬러 체이스",
        "유성", "반짝이", "그라데이션", "마키",
    ],
    sound_effects: &[
        "리듬 웨이브", "리듬 펄스", "리듬 스펙트럼",
        "리듬 플래시", "리듬 그라데이션", "리듬 체이스", "리듬 레인보우",
    ],
    color_presets: &[
        ("빨강", [255, 0, 0]),
        ("주황", [255, 120, 0]),
        ("노랑", [255, 255, 0]),
        ("초록", [0, 255, 0]),
        ("시안", [0, 255, 255]),
        ("파랑", [0, 0, 255]),
        ("보라", [150, 0, 255]),
        ("핑크", [255, 0, 150]),
        ("흰색", [255, 255, 255]),
        ("웜 화이트", [255, 180, 100]),
    ],
    tray_pause: "일시정지",
    tray_resume: "재개",
    tray_settings: "설정",
    tray_quit: "종료",
    tray_paused_tooltip: "SyncRGB (일시정지)",
    language_label: "언어",
    lang_auto: "자동",
};

static EN: Locale = Locale {
    tab_sync: "Screen Sync",
    tab_dynamic: "Dynamic Effects",
    tab_sound: "Sound Reactive",
    tab_static: "Static Color",
    led_on: "LED ON",
    led_off: "LED OFF",
    led_off_msg: "LEDs are turned off",
    led_off_hint: "Click a mode tab to turn LEDs on",
    brightness: "Brightness:",
    settings: "Settings",
    applied: "Applied",
    save_failed_prefix: "Save failed: ",
    failed_keyword: "failed",
    sync_speed: "Sync Speed",
    speed_fastest: "Fastest",
    speed_fast: "Fast",
    speed_normal: "Normal",
    speed_slow: "Slow",
    color_processing: "Color Processing",
    saturation: "Saturation",
    gamma: "Gamma",
    light_compression: "Light Compression (USB power opt.)",
    smoothing: "Smooth Transition (Smoothing)",
    led_layout: "LED Layout",
    direction: "Direction:",
    dir_ltr: "Left → Right",
    dir_rtl: "Right → Left",
    edges: "Edges:",
    edges_3: "3 sides (Top/Left/Right)",
    edges_4: "4 sides",
    pattern_hint: "Select a pattern to apply instantly",
    effect_speed: "Effect Speed",
    sound_source: "Sound Source",
    source_controller: "Controller (Built-in Mic)",
    source_computer: "Computer (PC Audio)",
    color_adjust: "Color Adjustment",
    quick_select: "Quick Select",
    static_animation: "Static Animation",
    anim_none: "None",
    anim_breathe: "Breathe",
    anim_rotate: "Rotate",
    speed: "Speed",
    device: "Device",
    led_count: "LED Count:",
    monitor_label: "Monitor:",
    capture_fps: "Capture FPS:",
    app_section: "App",
    autostart: "Start with Windows",
    autostart_failed_prefix: "Autostart failed: ",
    save_settings: "  Save Settings  ",
    dynamic_effects: &[
        "Rainbow Flow", "Breathing", "Color Chase",
        "Meteor", "Sparkle", "Gradient", "Marquee",
    ],
    sound_effects: &[
        "Rhythm Wave", "Rhythm Pulse", "Rhythm Spectrum",
        "Rhythm Flash", "Rhythm Gradient", "Rhythm Chase", "Rhythm Rainbow",
    ],
    color_presets: &[
        ("Red", [255, 0, 0]),
        ("Orange", [255, 120, 0]),
        ("Yellow", [255, 255, 0]),
        ("Green", [0, 255, 0]),
        ("Cyan", [0, 255, 255]),
        ("Blue", [0, 0, 255]),
        ("Purple", [150, 0, 255]),
        ("Pink", [255, 0, 150]),
        ("White", [255, 255, 255]),
        ("Warm White", [255, 180, 100]),
    ],
    tray_pause: "Pause",
    tray_resume: "Resume",
    tray_settings: "Settings",
    tray_quit: "Quit",
    tray_paused_tooltip: "SyncRGB (Paused)",
    language_label: "Language",
    lang_auto: "Auto",
};

pub fn resolve_locale(lang: &crate::config::Language) -> &'static Locale {
    use crate::config::Language;
    match lang {
        Language::Ko => &KO,
        Language::En => &EN,
        Language::Auto => {
            use windows::Win32::Globalization::GetUserDefaultUILanguage;
            let lang_id = unsafe { GetUserDefaultUILanguage() };
            if lang_id & 0x3FF == 0x12 { &KO } else { &EN }
        }
    }
}

pub fn detect_locale() -> &'static Locale {
    let config = crate::config::Config::load_or_default();
    resolve_locale(&config.app.language)
}

/// 설정 GUI를 별도 프로세스로 실행
pub fn open_settings() {
    if let Ok(exe) = std::env::current_exe() {
        if let Err(e) = std::process::Command::new(exe).arg("--settings").spawn() {
            log::error!("Failed to open settings GUI: {}", e);
        }
    }
}

/// RGB 스펙트럼 링 아이콘 RGBA 데이터 생성
pub fn generate_rgb_icon(size: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let c = size as f32 / 2.0;
    let outer_r = c - 0.5;
    let inner_r = outer_r * 0.5;
    let aa = if size <= 16 { 0.8 } else { 1.2 };

    for y in 0..size {
        for x in 0..size {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let dx = px - c;
            let dy = py - c;
            let dist = (dx * dx + dy * dy).sqrt();
            let idx = ((y * size + x) * 4) as usize;

            let outer_mask = icon_smoothstep(outer_r + aa, outer_r - aa, dist);
            let inner_mask = icon_smoothstep(inner_r - aa, inner_r + aa, dist);
            let alpha = outer_mask * inner_mask;

            if alpha > 0.001 {
                let angle = dx.atan2(-dy);
                let hue = (angle.to_degrees() + 360.0) % 360.0;
                let (r, g, b) = icon_hsl_to_rgb(hue, 1.0, 0.5);
                rgba[idx] = r;
                rgba[idx + 1] = g;
                rgba[idx + 2] = b;
                rgba[idx + 3] = (alpha * 255.0) as u8;
            }
        }
    }
    rgba
}

fn icon_smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn icon_hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    (
        ((r1 + m) * 255.0) as u8,
        ((g1 + m) * 255.0) as u8,
        ((b1 + m) * 255.0) as u8,
    )
}

/// --settings 모드 진입점
pub fn run_settings_window() {
    let locale = detect_locale();
    let config_path = Config::config_path();
    let config = Config::load(&config_path).unwrap_or_default();
    let app = SettingsGui::new(config, config_path, locale);

    let icon_data = generate_rgb_icon(32);
    let icon = egui::IconData { rgba: icon_data, width: 32, height: 32 };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([500.0, 620.0])
            .with_min_inner_size([460.0, 480.0])
            .with_resizable(true)
            .with_icon(icon),
        ..Default::default()
    };

    if let Err(e) = eframe::run_native(
        "SyncRGB",
        options,
        Box::new(|cc| {
            setup_fonts(&cc.egui_ctx);
            setup_style(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    ) {
        eprintln!("GUI failed: {}", e);
    }
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(data) = std::fs::read("C:\\Windows\\Fonts\\malgun.ttf") {
        fonts.font_data.insert("malgun".into(), egui::FontData::from_owned(data).into());
        fonts.families.get_mut(&egui::FontFamily::Proportional).unwrap().insert(0, "malgun".into());
    }
    ctx.set_fonts(fonts);
}

fn setup_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.visuals.widgets.inactive.rounding = egui::Rounding::same(6.0);
    style.visuals.widgets.hovered.rounding = egui::Rounding::same(6.0);
    style.visuals.widgets.active.rounding = egui::Rounding::same(6.0);
    ctx.set_style(style);
}

// ── 모드 탭 ──

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Sync,
    Dynamic,
    Sound,
    Static,
}

impl Tab {
    fn to_mode(self) -> LedMode {
        match self {
            Tab::Sync => LedMode::Sync,
            Tab::Dynamic => LedMode::Dynamic,
            Tab::Sound => LedMode::Sound,
            Tab::Static => LedMode::Static,
        }
    }

    fn from_mode(mode: &LedMode) -> Self {
        match mode {
            LedMode::Sync => Tab::Sync,
            LedMode::Dynamic => Tab::Dynamic,
            LedMode::Sound => Tab::Sound,
            LedMode::Static => Tab::Static,
            LedMode::Off => Tab::Sync,
        }
    }
}

// ── GUI 상태 ──

struct SettingsGui {
    config: Config,
    config_path: PathBuf,
    status_msg: String,
    status_time: f64,
    tab: Tab,
    led_on: bool,
    autostart: bool,
    show_settings: bool,
    locale: &'static Locale,
}

impl SettingsGui {
    fn new(config: Config, config_path: PathBuf, locale: &'static Locale) -> Self {
        let tab = Tab::from_mode(&config.effect.mode);
        let led_on = config.effect.mode != LedMode::Off;
        let autostart = config::is_autostart_enabled();
        Self {
            config, config_path,
            status_msg: String::new(), status_time: 0.0,
            tab, led_on, autostart,
            show_settings: false,
            locale,
        }
    }

    fn save_and_apply(&mut self, time: f64) {
        self.config.effect.mode = if self.led_on {
            self.tab.to_mode()
        } else {
            LedMode::Off
        };

        match self.config.save(&self.config_path) {
            Ok(()) => self.status_msg = self.locale.applied.into(),
            Err(e) => self.status_msg = format!("{}{}", self.locale.save_failed_prefix, e),
        }
        self.status_time = time;
    }
}

// ── eframe 메인 루프 ──

impl eframe::App for SettingsGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let accent = egui::Color32::from_rgb(0, 180, 255);
        let locale = self.locale;

        // ── 상단 헤더 ──
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                // RGB 스펙트럼 링 아이콘
                let (rect, _) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::hover());
                let c = rect.center();
                let segments = 24;
                let ring_outer = 11.0f32;
                let ring_inner = 6.0f32;
                for i in 0..segments {
                    let a0 = (i as f32 / segments as f32) * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
                    let a1 = ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
                    let hue = (i as f32 / segments as f32) * 360.0;
                    let (r, g, b) = icon_hsl_to_rgb(hue, 1.0, 0.5);
                    ui.painter().add(egui::Shape::convex_polygon(
                        vec![
                            egui::pos2(c.x + ring_inner * a0.cos(), c.y + ring_inner * a0.sin()),
                            egui::pos2(c.x + ring_outer * a0.cos(), c.y + ring_outer * a0.sin()),
                            egui::pos2(c.x + ring_outer * a1.cos(), c.y + ring_outer * a1.sin()),
                            egui::pos2(c.x + ring_inner * a1.cos(), c.y + ring_inner * a1.sin()),
                        ],
                        egui::Color32::from_rgb(r, g, b),
                        egui::Stroke::NONE,
                    ));
                }

                ui.heading(egui::RichText::new("SyncRGB").strong().size(20.0));

                // LED 켜기/끄기 토글
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (label, color) = if self.led_on {
                        (locale.led_on, egui::Color32::from_rgb(80, 220, 80))
                    } else {
                        (locale.led_off, egui::Color32::from_rgb(160, 160, 160))
                    };
                    let btn = egui::Button::new(
                        egui::RichText::new(label).color(color).strong().size(13.0),
                    );
                    if ui.add(btn).clicked() {
                        self.led_on = !self.led_on;
                        self.save_and_apply(ui.input(|i| i.time));
                    }
                });
            });

            ui.add_space(6.0);

            // ── 모드 탭 바 ──
            ui.horizontal(|ui| {
                let tabs = [
                    (Tab::Sync, locale.tab_sync),
                    (Tab::Dynamic, locale.tab_dynamic),
                    (Tab::Sound, locale.tab_sound),
                    (Tab::Static, locale.tab_static),
                ];
                for (t, label) in tabs {
                    let selected = self.tab == t && self.led_on;
                    let text = if selected {
                        egui::RichText::new(label).strong().size(14.0).color(accent)
                    } else {
                        egui::RichText::new(label).size(14.0)
                    };
                    if ui.selectable_label(selected, text).clicked() {
                        if !self.led_on { self.led_on = true; }
                        self.tab = t;
                        self.save_and_apply(ui.input(|i| i.time));
                    }
                }
            });
            ui.separator();
        });

        // ── 하단 바: 밝기 + 설정 ──
        egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(locale.brightness);
                let mut b = self.config.sync.brightness as f32;
                let resp = ui.add(egui::Slider::new(&mut b, 5.0..=255.0).show_value(false));
                self.config.sync.brightness = b as u8;
                ui.label(format!("{}", self.config.sync.brightness));
                if resp.drag_stopped() {
                    self.save_and_apply(ui.input(|i| i.time));
                }

                ui.separator();

                if ui.button(locale.settings).clicked() {
                    self.show_settings = !self.show_settings;
                }

                // 상태 메시지
                if !self.status_msg.is_empty() {
                    let elapsed = ui.input(|i| i.time) - self.status_time;
                    if elapsed < 2.0 {
                        let color = if self.status_msg.contains(locale.failed_keyword) {
                            egui::Color32::from_rgb(255, 100, 100)
                        } else {
                            egui::Color32::from_rgb(80, 200, 80)
                        };
                        ui.label(egui::RichText::new(&self.status_msg).color(color).size(12.0));
                    }
                }
            });
            ui.add_space(4.0);
        });

        // ── 메인 콘텐츠 ──
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                if !self.led_on {
                    ui.add_space(60.0);
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new(locale.led_off_msg).size(18.0).weak());
                        ui.add_space(8.0);
                        ui.label(locale.led_off_hint);
                    });
                } else {
                    match self.tab {
                        Tab::Sync => self.draw_sync_tab(ui),
                        Tab::Dynamic => self.draw_dynamic_tab(ui),
                        Tab::Sound => self.draw_sound_tab(ui),
                        Tab::Static => self.draw_static_tab(ui),
                    }
                }

                if self.show_settings {
                    ui.add_space(12.0);
                    ui.separator();
                    self.draw_settings_section(ui);
                }
            });
        });
    }
}

// ── 각 탭 UI ──

impl SettingsGui {
    fn draw_sync_tab(&mut self, ui: &mut egui::Ui) {
        let locale = self.locale;
        ui.add_space(4.0);

        section(ui, locale.sync_speed, |ui| {
            ui.horizontal(|ui| {
                for &(val, label) in &[
                    (0u32, locale.speed_fastest),
                    (10, locale.speed_fast),
                    (20, locale.speed_normal),
                    (100, locale.speed_slow),
                ] {
                    let selected = self.config.sync.speed == val;
                    let text = if selected {
                        egui::RichText::new(label).strong()
                    } else {
                        egui::RichText::new(label)
                    };
                    if ui.selectable_label(selected, text).clicked() {
                        self.config.sync.speed = val;
                        self.save_and_apply(ui.input(|i| i.time));
                    }
                }
            });
        });

        section(ui, locale.color_processing, |ui| {
            let mut changed = false;
            if ui.add(egui::Slider::new(&mut self.config.sync.saturation, 0.0..=3.0).text(locale.saturation)).drag_stopped() { changed = true; }
            if ui.add(egui::Slider::new(&mut self.config.sync.gamma, 0.1..=3.0).text(locale.gamma)).drag_stopped() { changed = true; }
            if ui.checkbox(&mut self.config.sync.light_compression, locale.light_compression).changed() { changed = true; }
            if ui.checkbox(&mut self.config.sync.smoothing, locale.smoothing).changed() { changed = true; }
            if changed { self.save_and_apply(ui.input(|i| i.time)); }
        });

        section(ui, locale.led_layout, |ui| {
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label(locale.direction);
                if ui.selectable_value(&mut self.config.sync.reverse, false, locale.dir_ltr).changed() { changed = true; }
                if ui.selectable_value(&mut self.config.sync.reverse, true, locale.dir_rtl).changed() { changed = true; }
            });
            ui.horizontal(|ui| {
                ui.label(locale.edges);
                if ui.selectable_value(&mut self.config.sync.edge_number, 3, locale.edges_3).changed() { changed = true; }
                if ui.selectable_value(&mut self.config.sync.edge_number, 4, locale.edges_4).changed() { changed = true; }
            });
            if changed { self.save_and_apply(ui.input(|i| i.time)); }
        });
    }

    fn draw_dynamic_tab(&mut self, ui: &mut egui::Ui) {
        let locale = self.locale;
        ui.add_space(4.0);
        ui.label(egui::RichText::new(locale.pattern_hint).size(12.0).weak());
        ui.add_space(4.0);

        let cols = 3;
        let colors = [
            egui::Color32::from_rgb(255, 50, 50),
            egui::Color32::from_rgb(50, 200, 50),
            egui::Color32::from_rgb(50, 100, 255),
            egui::Color32::from_rgb(255, 165, 0),
            egui::Color32::from_rgb(200, 50, 200),
            egui::Color32::from_rgb(0, 200, 200),
            egui::Color32::from_rgb(255, 215, 0),
        ];

        egui::Grid::new("dynamic_grid").num_columns(cols).spacing([10.0, 10.0]).show(ui, |ui| {
            for (i, name) in locale.dynamic_effects.iter().enumerate() {
                let selected = self.config.effect.dynamic_index == i as u8;
                if effect_button(ui, name, colors[i], selected) {
                    self.config.effect.dynamic_index = i as u8;
                    self.save_and_apply(ui.input(|i| i.time));
                }
                if (i + 1) % cols == 0 { ui.end_row(); }
            }
        });

        ui.add_space(12.0);
        section(ui, locale.effect_speed, |ui| {
            let resp = ui.add(egui::Slider::new(&mut self.config.effect.effect_speed, 0..=100));
            if resp.drag_stopped() {
                self.save_and_apply(ui.input(|i| i.time));
            }
        });
    }

    fn draw_sound_tab(&mut self, ui: &mut egui::Ui) {
        let locale = self.locale;
        ui.add_space(4.0);

        section(ui, locale.sound_source, |ui| {
            ui.horizontal(|ui| {
                let mut changed = false;
                if ui.selectable_value(&mut self.config.effect.rhythm_source, RhythmSource::Controller, locale.source_controller).changed() { changed = true; }
                if ui.selectable_value(&mut self.config.effect.rhythm_source, RhythmSource::Computer, locale.source_computer).changed() { changed = true; }
                if changed { self.save_and_apply(ui.input(|i| i.time)); }
            });
        });

        ui.label(egui::RichText::new(locale.pattern_hint).size(12.0).weak());
        ui.add_space(4.0);

        let cols = 3;
        let colors = [
            egui::Color32::from_rgb(255, 80, 80),
            egui::Color32::from_rgb(80, 255, 80),
            egui::Color32::from_rgb(80, 80, 255),
            egui::Color32::from_rgb(255, 200, 50),
            egui::Color32::from_rgb(200, 80, 255),
            egui::Color32::from_rgb(80, 220, 220),
            egui::Color32::from_rgb(255, 100, 200),
        ];

        egui::Grid::new("sound_grid").num_columns(cols).spacing([10.0, 10.0]).show(ui, |ui| {
            for (i, name) in locale.sound_effects.iter().enumerate() {
                let selected = self.config.effect.sound_index == i as u8;
                if effect_button(ui, name, colors[i], selected) {
                    self.config.effect.sound_index = i as u8;
                    self.save_and_apply(ui.input(|i| i.time));
                }
                if (i + 1) % cols == 0 { ui.end_row(); }
            }
        });
    }

    fn draw_static_tab(&mut self, ui: &mut egui::Ui) {
        let locale = self.locale;
        ui.add_space(4.0);

        // 색상 미리보기
        let preview_w = ui.available_width().min(420.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(preview_w, 50.0), egui::Sense::hover());
        let color = egui::Color32::from_rgb(
            self.config.effect.color_r, self.config.effect.color_g, self.config.effect.color_b,
        );
        ui.painter().rect_filled(rect, 8.0, color);
        let hex = format!("#{:02X}{:02X}{:02X}",
            self.config.effect.color_r, self.config.effect.color_g, self.config.effect.color_b);
        let text_c = if (self.config.effect.color_r as u16 + self.config.effect.color_g as u16 + self.config.effect.color_b as u16) > 384 {
            egui::Color32::BLACK
        } else {
            egui::Color32::WHITE
        };
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, &hex,
            egui::FontId::proportional(15.0), text_c);

        ui.add_space(10.0);

        // RGB 슬라이더
        let mut color_changed = false;
        section(ui, locale.color_adjust, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("R").color(egui::Color32::from_rgb(255, 80, 80)).strong());
                if ui.add(egui::Slider::new(&mut self.config.effect.color_r, 0..=255)).drag_stopped() { color_changed = true; }
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("G").color(egui::Color32::from_rgb(80, 200, 80)).strong());
                if ui.add(egui::Slider::new(&mut self.config.effect.color_g, 0..=255)).drag_stopped() { color_changed = true; }
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("B").color(egui::Color32::from_rgb(80, 120, 255)).strong());
                if ui.add(egui::Slider::new(&mut self.config.effect.color_b, 0..=255)).drag_stopped() { color_changed = true; }
            });
        });

        // 프리셋 색상
        section(ui, locale.quick_select, |ui| {
            ui.horizontal_wrapped(|ui| {
                for &(name, [r, g, b]) in locale.color_presets {
                    let btn_color = egui::Color32::from_rgb(r, g, b);
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(38.0, 26.0), egui::Sense::click());
                    ui.painter().rect_filled(rect, 4.0, btn_color);
                    let is_sel = self.config.effect.color_r == r
                        && self.config.effect.color_g == g
                        && self.config.effect.color_b == b;
                    if resp.hovered() || is_sel {
                        let sc = if is_sel { egui::Color32::from_rgb(0, 180, 255) } else { egui::Color32::WHITE };
                        ui.painter().rect_stroke(rect, 4.0, egui::Stroke::new(2.0, sc));
                    }
                    if resp.clicked() {
                        self.config.effect.color_r = r;
                        self.config.effect.color_g = g;
                        self.config.effect.color_b = b;
                        color_changed = true;
                    }
                    resp.on_hover_text(name);
                }
            });
        });

        if color_changed {
            self.save_and_apply(ui.input(|i| i.time));
        }

        // 소프트웨어 동적 효과
        section(ui, locale.static_animation, |ui| {
            ui.horizontal(|ui| {
                let mut changed = false;
                if ui.selectable_value(&mut self.config.effect.soft_effect, SoftEffect::None, locale.anim_none).changed() { changed = true; }
                if ui.selectable_value(&mut self.config.effect.soft_effect, SoftEffect::Breathe, locale.anim_breathe).changed() { changed = true; }
                if ui.selectable_value(&mut self.config.effect.soft_effect, SoftEffect::Rotate, locale.anim_rotate).changed() { changed = true; }
                if changed { self.save_and_apply(ui.input(|i| i.time)); }
            });
            if self.config.effect.soft_effect != SoftEffect::None {
                let resp = ui.add(egui::Slider::new(&mut self.config.effect.effect_speed, 5..=100).text(locale.speed));
                if resp.drag_stopped() {
                    self.save_and_apply(ui.input(|i| i.time));
                }
            }
        });
    }

    fn draw_settings_section(&mut self, ui: &mut egui::Ui) {
        let locale = self.locale;

        section(ui, locale.device, |ui| {
            ui.horizontal(|ui| {
                ui.label(locale.led_count);
                ui.add(egui::DragValue::new(&mut self.config.device.lamps_amount).range(1..=254));
            });
            egui::ComboBox::from_label("Wire Map")
                .selected_text(&self.config.device.wire_map)
                .show_ui(ui, |ui| {
                    for &wm in WIRE_MAPS {
                        ui.selectable_value(&mut self.config.device.wire_map, wm.to_string(), wm);
                    }
                });
            ui.horizontal(|ui| {
                ui.label(locale.monitor_label);
                ui.add(egui::DragValue::new(&mut self.config.capture.monitor).range(0..=4));
            });
            ui.horizontal(|ui| {
                ui.label(locale.capture_fps);
                ui.add(egui::DragValue::new(&mut self.config.capture.fps).range(10..=60));
            });
        });

        section(ui, locale.app_section, |ui| {
            ui.horizontal(|ui| {
                ui.label(locale.language_label);
                let lang = &mut self.config.app.language;
                let mut changed = false;
                if ui.selectable_value(lang, config::Language::Auto, locale.lang_auto).changed() { changed = true; }
                if ui.selectable_value(lang, config::Language::En, "English").changed() { changed = true; }
                if ui.selectable_value(lang, config::Language::Ko, "한국어").changed() { changed = true; }
                if changed {
                    self.locale = resolve_locale(&self.config.app.language);
                    self.save_and_apply(ui.input(|i| i.time));
                }
            });
            if ui.checkbox(&mut self.autostart, locale.autostart).changed() {
                if let Err(e) = config::set_autostart(self.autostart) {
                    self.status_msg = format!("{}{}", locale.autostart_failed_prefix, e);
                }
            }
        });

        ui.horizontal(|ui| {
            let btn = egui::Button::new(egui::RichText::new(locale.save_settings).size(13.0))
                .fill(egui::Color32::from_rgb(0, 140, 210));
            if ui.add(btn).clicked() {
                self.save_and_apply(ui.input(|i| i.time));
            }
        });
    }
}

// ── 공용 위젯 ──

fn section(ui: &mut egui::Ui, title: &str, content: impl FnOnce(&mut egui::Ui)) {
    ui.group(|ui| {
        ui.label(egui::RichText::new(title).strong().size(13.0));
        ui.separator();
        content(ui);
    });
    ui.add_space(4.0);
}

fn effect_button(ui: &mut egui::Ui, name: &str, color: egui::Color32, selected: bool) -> bool {
    let size = egui::vec2(130.0, 48.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());

    let bg = if selected {
        color.linear_multiply(0.3)
    } else if resp.hovered() {
        color.linear_multiply(0.15)
    } else {
        egui::Color32::from_gray(40)
    };

    ui.painter().rect_filled(rect, 8.0, bg);
    if selected {
        ui.painter().rect_stroke(rect, 8.0, egui::Stroke::new(2.0, color));
    }

    // 상단 색상 바
    let bar = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 3.0));
    ui.painter().rect_filled(
        bar,
        egui::Rounding { nw: 8.0, ne: 8.0, sw: 0.0, se: 0.0 },
        color,
    );

    let text_color = if selected { color } else { egui::Color32::from_gray(200) };
    ui.painter().text(
        rect.center() + egui::vec2(0.0, 2.0),
        egui::Align2::CENTER_CENTER,
        name,
        egui::FontId::proportional(12.0),
        text_color,
    );

    resp.clicked()
}
