// 콘솔 창 숨기기 (--console 플래그로 표시 가능)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod capture;
mod color;
mod config;
mod device;
mod gui;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::WindowId;

use capture::dxgi::{CaptureError, ScreenCapture};
use color::extractor::ColorExtractor;
use config::{Config, LedMode, RhythmSource, SoftEffect};
use device::protocol::WireMap;
use device::serial::DeviceConnection;

fn main() {
    // --console 플래그: 릴리즈에서도 콘솔 표시
    if std::env::args().any(|a| a == "--console") {
        unsafe { attach_console(); }
    }

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    if std::env::args().any(|a| a == "--settings") {
        gui::run_settings_window();
        return;
    }

    log::info!("SyncRGB 시작");

    let locale = gui::detect_locale();
    let config = Config::load_or_default();
    log::info!("설정: {:?}", config);

    let running = Arc::new(AtomicBool::new(true));
    let active = Arc::new(AtomicBool::new(true));
    let config_version = Arc::new(AtomicU32::new(0));

    spawn_shutdown_listener(running.clone());

    let (tx, rx) = mpsc::sync_channel::<Vec<(u8, u8, u8)>>(2);

    let capture_thread = {
        let running = running.clone();
        let active = active.clone();
        let config_version = config_version.clone();
        let config = config.clone();
        std::thread::Builder::new()
            .name("capture".into())
            .spawn(move || capture_loop(config, running, active, config_version, tx))
            .expect("캡처 스레드 생성 실패")
    };

    let send_thread = {
        let running = running.clone();
        let config_version = config_version.clone();
        let config = config.clone();
        std::thread::Builder::new()
            .name("sender".into())
            .spawn(move || sender_loop(config, running, config_version, rx))
            .expect("전송 스레드 생성 실패")
    };

    run_tray(running.clone(), active, config_version, locale);

    // 트레이 종료 후 스레드 정리 (정상 종료 경로)
    running.store(false, Ordering::SeqCst);
    let _ = capture_thread.join();
    let _ = send_thread.join();
    log::info!("SyncRGB 종료");
}

/// 릴리즈 빌드에서 콘솔 붙이기
#[allow(unused_unsafe)]
unsafe fn attach_console() {
    #[cfg(not(debug_assertions))]
    {
        use windows::Win32::System::Console::{AttachConsole, AllocConsole, ATTACH_PARENT_PROCESS};
        if AttachConsole(ATTACH_PARENT_PROCESS).is_err() {
            let _ = AllocConsole();
        }
    }
}

fn capture_loop(
    config: Config,
    running: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    config_version: Arc<AtomicU32>,
    tx: mpsc::SyncSender<Vec<(u8, u8, u8)>>,
) {
    let mut frame_interval = Duration::from_millis(1000 / config.capture.fps as u64);

    let mut capturer = match ScreenCapture::new(config.capture.monitor) {
        Ok(c) => c,
        Err(e) => {
            log::error!("화면 캡처 초기화 실패: {}", e);
            return;
        }
    };

    let mut extractor = ColorExtractor::new(
        config.device.lamps_amount,
        config.capture.sample_width,
        config.sync.gamma,
        config.sync.saturation,
        config.sync.light_compression,
        config.sync.smoothing,
        config.sync.reverse,
        config.sync.edge_number,
    );

    log::info!("캡처 스레드 시작 ({}fps, {}x{})", config.capture.fps, capturer.width, capturer.height);
    let mut local_version = 0u32;

    while running.load(Ordering::Relaxed) {
        let frame_start = Instant::now();

        let current_version = config_version.load(Ordering::Relaxed);
        if current_version != local_version {
            local_version = current_version;
            let c = Config::load_or_default();
            frame_interval = Duration::from_millis(1000 / c.capture.fps as u64);
            extractor.update_config(
                c.device.lamps_amount, c.sync.gamma, c.sync.saturation,
                c.sync.light_compression, c.sync.smoothing, c.sync.reverse, c.sync.edge_number,
            );
        }

        if !active.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }

        match capturer.capture_frame() {
            Ok((data, pitch)) => {
                let colors = extractor.extract(&data, pitch, capturer.width, capturer.height);
                let _ = tx.try_send(colors);
            }
            Err(CaptureError::Timeout) => {}
            Err(CaptureError::AccessLost) => {
                log::warn!("Desktop Duplication 접근 손실, 재초기화");
                std::thread::sleep(Duration::from_secs(1));
                if let Err(e) = capturer.reinitialize(Config::load_or_default().capture.monitor) {
                    log::error!("재초기화 실패: {}", e);
                }
            }
            Err(e) => {
                log::error!("캡처 오류: {}", e);
                std::thread::sleep(Duration::from_millis(100));
            }
        }

        let elapsed = frame_start.elapsed();
        if elapsed < frame_interval {
            std::thread::sleep(frame_interval - elapsed);
        }
    }
}

fn sender_loop(
    config: Config,
    running: Arc<AtomicBool>,
    config_version: Arc<AtomicU32>,
    rx: mpsc::Receiver<Vec<(u8, u8, u8)>>,
) {
    let mut wire_map = WireMap::from_str(&config.device.wire_map);
    let mut send_interval = Duration::from_millis(config.sync.interval_ms());
    let mut brightness = config.sync.brightness;
    let mut saturation_on = config.sync.saturation > 0.0;
    let mut light_compression = config.sync.light_compression;
    let mut current_mode = config.effect.mode.clone();
    let mut effect_cfg = config.effect.clone();

    // 디바이스 연결 (재시도)
    let mut conn = loop {
        if !running.load(Ordering::Relaxed) { return; }
        match DeviceConnection::connect(&config.device.com_port) {
            Ok(mut c) => {
                if let Err(e) = c.init_device() { log::warn!("초기화 실패: {}", e); }
                if let Err(e) = c.set_brightness(brightness) { log::warn!("밝기 실패: {}", e); }
                log::info!("디바이스 연결 (MAC={:02x?})", c.mac);
                break c;
            }
            Err(e) => {
                log::warn!("연결 실패: {} — 3초 후 재시도", e);
                std::thread::sleep(Duration::from_secs(3));
            }
        }
    };

    let mut lamps_amount = config.device.lamps_amount;
    apply_mode(&conn, &effect_cfg, lamps_amount);
    let mut local_version = 0u32;
    let mut send_count = 0u64;

    // 컴퓨터 리듬용 오디오 미터 (필요 시 초기화)
    let mut audio_meter: Option<audio::AudioMeter> = None;
    // 소프트웨어 효과 타이머
    let mut soft_tick: f64 = 0.0;

    while running.load(Ordering::Relaxed) {
        let send_start = Instant::now();

        let cv = config_version.load(Ordering::Relaxed);
        if cv != local_version {
            local_version = cv;
            let c = Config::load_or_default();
            wire_map = WireMap::from_str(&c.device.wire_map);
            send_interval = Duration::from_millis(c.sync.interval_ms());
            saturation_on = c.sync.saturation > 0.0;
            light_compression = c.sync.light_compression;

            if brightness != c.sync.brightness {
                brightness = c.sync.brightness;
                let _ = conn.set_brightness(brightness);
            }

            lamps_amount = c.device.lamps_amount;

            if current_mode != c.effect.mode || effect_cfg_changed(&effect_cfg, &c.effect) {
                current_mode = c.effect.mode.clone();
                effect_cfg = c.effect.clone();
                apply_mode(&conn, &effect_cfg, lamps_amount);
            }
        }

        if current_mode == LedMode::Sync {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(colors) => {
                    let mut color_data = Vec::with_capacity(colors.len() * 5);
                    for (i, &(r, g, b)) in colors.iter().enumerate() {
                        let mapped = wire_map.apply(r, g, b);
                        let (cr, cg, cb) = convert_to_black(mapped[0], mapped[1], mapped[2], 20);
                        let idx = (i + 1) as u8;
                        color_data.push(idx);
                        color_data.push(cr);
                        color_data.push(cg);
                        color_data.push(cb);
                        color_data.push(idx);
                    }
                    low_light_for_sync(&mut color_data, saturation_on, light_compression);

                    if send_count == 0 {
                        log::info!("첫 SC 패킷: {}LED, {}바이트", color_data.len() / 5, color_data.len());
                    }
                    send_count += 1;

                    if let Err(e) = conn.set_sync_screen(&color_data) {
                        log::error!("전송 실패: {} — 재연결", e);
                        std::thread::sleep(Duration::from_secs(1));
                        if let Ok(mut c) = DeviceConnection::connect(&Config::load_or_default().device.com_port) {
                            let _ = c.init_device();
                            conn = c;
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        } else if current_mode == LedMode::Sound && effect_cfg.rhythm_source == RhythmSource::Computer {
            // 컴퓨터 리듬: 오디오 볼륨 → setComputerRhythm 반복
            if audio_meter.is_none() {
                audio_meter = audio::AudioMeter::new().ok();
                if audio_meter.is_none() {
                    log::warn!("오디오 미터 초기화 실패");
                }
            }
            if let Some(ref mut meter) = audio_meter {
                let vol = meter.peak_volume();
                let _ = conn.set_computer_rhythm(effect_cfg.sound_index, vol);
            }
            while rx.try_recv().is_ok() {}
            std::thread::sleep(Duration::from_millis(40));
            continue;
        } else if current_mode == LedMode::Static && effect_cfg.soft_effect != SoftEffect::None {
            // 소프트웨어 동적 효과 (단색 숨쉬기/회전)
            soft_tick += 0.05;
            let speed_factor = effect_cfg.effect_speed as f64 / 50.0;
            let t = soft_tick * speed_factor;

            match effect_cfg.soft_effect {
                SoftEffect::Breathe => {
                    // 사인파로 밝기 변화 (0.05 ~ 1.0)
                    let brightness = ((t.sin() + 1.0) / 2.0 * 0.95 + 0.05) as f32;
                    let r = (effect_cfg.color_r as f32 * brightness) as u8;
                    let g = (effect_cfg.color_g as f32 * brightness) as u8;
                    let b = (effect_cfg.color_b as f32 * brightness) as u8;
                    let _ = conn.set_section_led(r, g, b, lamps_amount);
                }
                SoftEffect::Rotate => {
                    // LED 위치별 그라데이션 회전
                    let n = lamps_amount as usize;
                    let mut data = Vec::with_capacity(n * 5);
                    for i in 0..n {
                        let phase = (i as f64 / n as f64 + t * 0.1) % 1.0;
                        let brightness = ((phase * std::f64::consts::TAU).sin() + 1.0) / 2.0;
                        let r = (effect_cfg.color_r as f64 * brightness) as u8;
                        let g = (effect_cfg.color_g as f64 * brightness) as u8;
                        let b = (effect_cfg.color_b as f64 * brightness) as u8;
                        let mapped = wire_map.apply(r, g, b);
                        let idx = (i + 1) as u8;
                        data.push(idx);
                        data.push(mapped[0]);
                        data.push(mapped[1]);
                        data.push(mapped[2]);
                        data.push(idx);
                    }
                    let _ = conn.set_sync_screen(&data);
                }
                SoftEffect::None => {}
            }
            while rx.try_recv().is_ok() {}
            std::thread::sleep(Duration::from_millis(30));
            continue;
        } else {
            // 하드웨어 동적/음악/단색/끄기 — 별도 루프 불필요
            std::thread::sleep(Duration::from_millis(200));
            while rx.try_recv().is_ok() {}
            continue;
        }

        let elapsed = send_start.elapsed();
        if elapsed < send_interval {
            std::thread::sleep(send_interval - elapsed);
        }
    }

    log::info!("앱 종료 — LED 끄기");
    let _ = conn.turn_off();
}

fn effect_cfg_changed(a: &config::EffectConfig, b: &config::EffectConfig) -> bool {
    a.dynamic_index != b.dynamic_index
        || a.sound_index != b.sound_index
        || a.color_r != b.color_r
        || a.color_g != b.color_g
        || a.color_b != b.color_b
        || a.effect_speed != b.effect_speed
        || a.rhythm_source != b.rhythm_source
        || a.soft_effect != b.soft_effect
}

fn apply_mode(conn: &DeviceConnection, effect: &config::EffectConfig, lamps_amount: u32) {
    log::info!("모드 적용: {:?}", effect.mode);
    let result = match effect.mode {
        LedMode::Sync => {
            // 동기화 모드: 별도 명령 불필요, sender_loop이 SC 패킷 전송
            Ok(())
        }
        LedMode::Dynamic => {
            // 원본 흐름: setSectionLED → setLedEffect(2=동적, index)
            conn.set_section_led(0, 0, 0, lamps_amount).ok();
            std::thread::sleep(Duration::from_millis(20));
            if let Err(e) = conn.set_led_effect(2, effect.dynamic_index) {
                log::warn!("동적 효과 설정 실패: {}", e);
            }
            std::thread::sleep(Duration::from_millis(80));
            conn.set_dynamic_speed(effect.effect_speed)
        }
        LedMode::Sound => {
            // 원본 흐름: setSectionLED → setLedEffect(3=음악, index)
            conn.set_section_led(0, 0, 0, lamps_amount).ok();
            std::thread::sleep(Duration::from_millis(20));
            conn.set_led_effect(3, effect.sound_index)
        }
        LedMode::Static => {
            // setSectionLED로 단색 전송
            conn.set_section_led(effect.color_r, effect.color_g, effect.color_b, lamps_amount)
        }
        LedMode::Off => {
            conn.turn_off()
        }
    };
    if let Err(e) = result {
        log::warn!("모드 적용 실패: {}", e);
    }
}

fn convert_to_black(r: u8, g: u8, b: u8, threshold: u8) -> (u8, u8, u8) {
    if r <= threshold && g <= threshold && b <= threshold {
        return (0, 0, 0);
    }
    let mut out = [r, g, b];
    let mut dominant = None;
    let mut weak_count = 0u8;
    let mut weak = [false; 3];
    for (i, &v) in out.iter().enumerate() {
        if v >= 200 { dominant = Some(i); }
        if v <= 50 { weak[i] = true; weak_count += 1; }
    }
    if let Some(d) = dominant {
        if weak_count == 2 {
            out[d] = ((out[d] as u16 * 3) / 2).min(255) as u8;
            for i in 0..3 {
                if weak[i] { out[i] = 0; }
            }
        }
    }
    (out[0], out[1], out[2])
}

fn boost_saturation_3x(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let (rf, gf, bf) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let l = (max + min) / 2.0;
    if (max - min).abs() < 0.001 { return (r, g, b); }

    let delta = max - min;
    let s = if l > 0.5 { delta / (2.0 - max - min) } else { delta / (max + min) };
    let mut h = if (max - rf).abs() < 0.001 {
        (gf - bf) / delta + if gf < bf { 6.0 } else { 0.0 }
    } else if (max - gf).abs() < 0.001 {
        (bf - rf) / delta + 2.0
    } else {
        (rf - gf) / delta + 4.0
    };
    h /= 6.0;
    let new_s = (s * 3.0).min(1.0);

    let hue_to_rgb = |p: f32, q: f32, mut t: f32| -> f32 {
        if t < 0.0 { t += 1.0; }
        if t > 1.0 { t -= 1.0; }
        if t < 1.0 / 6.0 { return p + (q - p) * 6.0 * t; }
        if t < 0.5 { return q; }
        if t < 2.0 / 3.0 { return p + (q - p) * (2.0 / 3.0 - t) * 6.0; }
        p
    };

    let (ro, go, bo) = if new_s.abs() < 0.001 { (l, l, l) } else {
        let q = if l < 0.5 { l * (1.0 + new_s) } else { l + new_s - l * new_s };
        let p = 2.0 * l - q;
        (hue_to_rgb(p, q, h + 1.0/3.0), hue_to_rgb(p, q, h), hue_to_rgb(p, q, h - 1.0/3.0))
    };

    (
        (ro * 255.0).round().clamp(0.0, 255.0) as u8,
        (go * 255.0).round().clamp(0.0, 255.0) as u8,
        (bo * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

fn low_light_for_sync(data: &mut [u8], saturation: bool, compression: bool) {
    let mut i = 0;
    while i + 4 < data.len() {
        let orig_r = data[i + 1];
        let orig_g = data[i + 2];
        let orig_b = data[i + 3];

        if saturation {
            let (sr, sg, sb) = boost_saturation_3x(orig_r, orig_g, orig_b);
            data[i + 1] = sr;
            data[i + 2] = sg;
            data[i + 3] = sb;
        }

        let orig_sum = orig_r as u16 + orig_g as u16 + orig_b as u16;
        if compression && orig_sum > 255 {
            let ratio = orig_sum as f32 / 255.0;
            data[i + 1] = (orig_r as f32 / ratio) as u8;
            data[i + 2] = (orig_g as f32 / ratio) as u8;
            data[i + 3] = (orig_b as f32 / ratio) as u8;
        }
        i += 5;
    }
}

fn create_rgb_tray_icon() -> Icon {
    let size = 32u32;
    let rgba = gui::generate_rgb_icon(size);
    Icon::from_rgba(rgba, size, size).expect("아이콘 생성 실패")
}

fn create_gray_tray_icon() -> Icon {
    let size = 32u32;
    let rgba = gui::generate_rgb_icon(size);
    let gray: Vec<u8> = rgba
        .chunks(4)
        .flat_map(|px| {
            let lum = (0.299 * px[0] as f32 + 0.587 * px[1] as f32 + 0.114 * px[2] as f32) as u8;
            [lum, lum, lum, px[3] / 2]
        })
        .collect();
    Icon::from_rgba(gray, size, size).expect("아이콘 생성 실패")
}

// ── Windows 종료 감지 (WM_ENDSESSION) ──

static SHUTDOWN_FLAG: OnceLock<Arc<AtomicBool>> = OnceLock::new();

fn spawn_shutdown_listener(running: Arc<AtomicBool>) {
    SHUTDOWN_FLAG.set(running).ok();
    std::thread::Builder::new()
        .name("shutdown".into())
        .spawn(|| unsafe {
            use windows::Win32::UI::WindowsAndMessaging::*;

            let class_name = windows::core::w!("SyncRGB_Shutdown");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(shutdown_wnd_proc),
                lpszClassName: class_name,
                ..Default::default()
            };
            RegisterClassW(&wc);

            // 숨겨진 top-level 윈도우 (WM_ENDSESSION 수신용)
            let _ = CreateWindowExW(
                WINDOW_EX_STYLE(0), class_name,
                windows::core::w!(""), WINDOW_STYLE(0),
                0, 0, 0, 0,
                None, None, None, None,
            );

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).into() {
                DispatchMessageW(&msg);
            }
        })
        .ok();
}

unsafe extern "system" fn shutdown_wnd_proc(
    hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM,
) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::*;
    match msg {
        WM_QUERYENDSESSION => LRESULT(1),
        WM_ENDSESSION if wparam.0 != 0 => {
            if let Some(running) = SHUTDOWN_FLAG.get() {
                running.store(false, Ordering::SeqCst);
                // sender_loop이 turn_off() 호출할 시간 확보
                std::thread::sleep(Duration::from_millis(800));
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn run_tray(running: Arc<AtomicBool>, active: Arc<AtomicBool>, config_version: Arc<AtomicU32>, locale: &'static gui::Locale) {
    let event_loop = EventLoop::new().expect("이벤트 루프 생성 실패");
    let mut app = TrayApp {
        running, active, config_version,
        config_path: Config::config_path(),
        last_mtime: None, tick_counter: 0,
        tray: None, toggle_item: None, settings_item: None, quit_item: None,
        locale,
    };
    event_loop.run_app(&mut app).ok();
}

struct TrayApp {
    running: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    config_version: Arc<AtomicU32>,
    config_path: PathBuf,
    last_mtime: Option<SystemTime>,
    tick_counter: u32,
    tray: Option<TrayIcon>,
    toggle_item: Option<MenuItem>,
    settings_item: Option<MenuItem>,
    quit_item: Option<MenuItem>,
    locale: &'static gui::Locale,
}

impl ApplicationHandler for TrayApp {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
        if self.tray.is_some() { return; }

        let menu = Menu::new();
        let toggle = MenuItem::new(self.locale.tray_pause, true, None);
        let settings = MenuItem::new(self.locale.tray_settings, true, None);
        let quit = MenuItem::new(self.locale.tray_quit, true, None);
        menu.append(&toggle).ok();
        menu.append(&settings).ok();
        menu.append(&PredefinedMenuItem::separator()).ok();
        menu.append(&quit).ok();

        let tray = TrayIconBuilder::new()
            .with_icon(create_rgb_tray_icon())
            .with_tooltip("SyncRGB")
            .with_menu(Box::new(menu))
            .build()
            .expect("트레이 아이콘 생성 실패");

        self.toggle_item = Some(toggle);
        self.settings_item = Some(settings);
        self.quit_item = Some(quit);
        self.tray = Some(tray);
        self.last_mtime = std::fs::metadata(&self.config_path).ok().and_then(|m| m.modified().ok());
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some(ref toggle) = self.toggle_item {
                if event.id() == toggle.id() {
                    let was = self.active.load(Ordering::Relaxed);
                    self.active.store(!was, Ordering::Relaxed);
                    if was {
                        toggle.set_text(self.locale.tray_resume);
                        if let Some(ref tray) = self.tray {
                            tray.set_icon(Some(create_gray_tray_icon())).ok();
                            tray.set_tooltip(Some(self.locale.tray_paused_tooltip)).ok();
                        }
                    } else {
                        toggle.set_text(self.locale.tray_pause);
                        if let Some(ref tray) = self.tray {
                            tray.set_icon(Some(create_rgb_tray_icon())).ok();
                            tray.set_tooltip(Some("SyncRGB")).ok();
                        }
                    }
                }
            }
            if let Some(ref s) = self.settings_item {
                if event.id() == s.id() { gui::open_settings(); }
            }
            if let Some(ref q) = self.quit_item {
                if event.id() == q.id() {
                    self.running.store(false, Ordering::Relaxed);
                    event_loop.exit();
                }
            }
        }

        self.tick_counter += 1;
        if self.tick_counter % 100 == 0 {
            if let Ok(meta) = std::fs::metadata(&self.config_path) {
                if let Ok(mtime) = meta.modified() {
                    if self.last_mtime.map_or(true, |prev| mtime != prev) {
                        self.last_mtime = Some(mtime);
                        self.config_version.fetch_add(1, Ordering::Relaxed);

                        let new_locale = gui::detect_locale();
                        if new_locale as *const _ != self.locale as *const _ {
                            self.locale = new_locale;
                            if let Some(ref t) = self.toggle_item {
                                if self.active.load(Ordering::Relaxed) {
                                    t.set_text(self.locale.tray_pause);
                                } else {
                                    t.set_text(self.locale.tray_resume);
                                }
                            }
                            if let Some(ref s) = self.settings_item {
                                s.set_text(self.locale.tray_settings);
                            }
                            if let Some(ref q) = self.quit_item {
                                q.set_text(self.locale.tray_quit);
                            }
                        }
                    }
                }
            }
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}
