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

    init_logging();

    // panic=abort + 콘솔 없음 조합에서 패닉이 흔적 없이 사라지는 것 방지
    std::panic::set_hook(Box::new(|info| {
        log::error!("패닉: {}", info);
    }));

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
    let monitor_off = Arc::new(AtomicBool::new(false));
    let screen_black = Arc::new(AtomicBool::new(false));
    // 절전·종료 전환 핸드셰이크: 리스너가 요청(suspend_pending) → sender 가 LED 끄고 핸들 해제(device_released)
    let suspend_pending = Arc::new(AtomicBool::new(false));
    let device_released = Arc::new(AtomicBool::new(false));

    spawn_shutdown_listener(running.clone(), monitor_off.clone(), suspend_pending.clone(), device_released.clone());

    let (tx, rx) = mpsc::sync_channel::<Vec<(u8, u8, u8)>>(2);

    let capture_thread = {
        let running = running.clone();
        let active = active.clone();
        let config_version = config_version.clone();
        let config = config.clone();
        let screen_black = screen_black.clone();
        std::thread::Builder::new()
            .name("capture".into())
            .spawn(move || capture_loop(config, running, active, config_version, tx, screen_black))
            .expect("캡처 스레드 생성 실패")
    };

    let send_thread = {
        let running = running.clone();
        let config_version = config_version.clone();
        let config = config.clone();
        let monitor_off = monitor_off.clone();
        let screen_black = screen_black.clone();
        let suspend_pending = suspend_pending.clone();
        let device_released = device_released.clone();
        std::thread::Builder::new()
            .name("sender".into())
            .spawn(move || sender_loop(config, running, config_version, rx, monitor_off, screen_black, suspend_pending, device_released))
            .expect("전송 스레드 생성 실패")
    };

    run_tray(running.clone(), active, config_version, locale);

    // 트레이 종료 후 스레드 정리 (정상 종료 경로)
    running.store(false, Ordering::SeqCst);
    let _ = capture_thread.join();
    let _ = send_thread.join();
    log::info!("SyncRGB 종료");
}

/// 로거 초기화. windows_subsystem="windows" 빌드에서 stderr는 유실되므로
/// exe 옆 syncrgb.log에 기록한다 (--console 시에는 기존대로 콘솔 출력).
fn init_logging() {
    let mut builder = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"));
    builder.format_timestamp_millis();

    if !std::env::args().any(|a| a == "--console") {
        if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
            let path = dir.join("syncrgb.log");
            if std::fs::metadata(&path).map_or(false, |m| m.len() > 1_000_000) {
                let _ = std::fs::rename(&path, dir.join("syncrgb.log.old"));
            }
            if let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                builder.target(env_logger::Target::Pipe(Box::new(file)));
            }
        }
    }
    builder.init();
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
    screen_black: Arc<AtomicBool>,
) {
    let mut frame_interval = Duration::from_millis(1000 / config.capture.fps as u64);
    let mut turn_off_on_black = config.app.turn_off_on_black;

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
    // 검정 화면 연속 프레임 카운터 (1초 이상 검정이면 플래그 설정)
    let mut black_frame_count = 0u32;
    let mut black_threshold_frames = config.capture.fps; // ~1초
    // 연속 에러 카운터 (누적 시 DXGI 재초기화)
    let mut consecutive_errors = 0u32;
    // 접근 손실(전체화면 독점 게임·보안 데스크톱)은 1초마다 반복된다 — 시작·복구만 남긴다
    // (2026-10-14 로그: 5시간 동안 19,355회 반복돼 7.5MB)
    let mut access_lost_since: Option<Instant> = None;
    let mut access_lost_reinits = 0u32;

    while running.load(Ordering::Relaxed) {
        let frame_start = Instant::now();

        let current_version = config_version.load(Ordering::Relaxed);
        if current_version != local_version {
            local_version = current_version;
            let c = Config::load_or_default();
            frame_interval = Duration::from_millis(1000 / c.capture.fps as u64);
            turn_off_on_black = c.app.turn_off_on_black;
            black_threshold_frames = c.capture.fps;
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
                consecutive_errors = 0;
                if let Some(since) = access_lost_since.take() {
                    log::info!("Desktop Duplication 접근 복구 ({}초, 재초기화 {}회)",
                        since.elapsed().as_secs(), access_lost_reinits);
                    access_lost_reinits = 0;
                }
                let colors = extractor.extract(&data, pitch, capturer.width, capturer.height);

                // 검정 화면 감지: 모든 LED 색상이 거의 검정인지 확인
                const BLACK_SUM_THRESHOLD: u16 = 15;
                if turn_off_on_black {
                    let is_black = colors.iter().all(|&(r, g, b)| {
                        (r as u16 + g as u16 + b as u16) < BLACK_SUM_THRESHOLD
                    });
                    if is_black {
                        black_frame_count = black_frame_count.saturating_add(1);
                    } else {
                        if black_frame_count >= black_threshold_frames {
                            log::info!("검정 화면 해제 — LED 복원");
                        }
                        black_frame_count = 0;
                    }
                    let was_black = screen_black.load(Ordering::Relaxed);
                    let now_black = black_frame_count >= black_threshold_frames;
                    if now_black != was_black {
                        screen_black.store(now_black, Ordering::SeqCst);
                        if now_black {
                            log::info!("검정 화면 감지 — LED 끄기");
                        }
                    }
                } else {
                    if screen_black.load(Ordering::Relaxed) {
                        screen_black.store(false, Ordering::SeqCst);
                    }
                    black_frame_count = 0;
                }

                let _ = tx.try_send(colors);
            }
            Err(CaptureError::Timeout) => {}
            Err(CaptureError::AccessLost) => {
                if access_lost_since.is_none() {
                    log::warn!("Desktop Duplication 접근 손실 — 복구될 때까지 1초 간격 재초기화");
                    access_lost_since = Some(Instant::now());
                }
                std::thread::sleep(Duration::from_secs(1));
                access_lost_reinits += 1;
                if let Err(e) = capturer.reinitialize(Config::load_or_default().capture.monitor) {
                    if access_lost_reinits == 1 || access_lost_reinits % 300 == 0 {
                        log::error!("재초기화 실패 ({}회): {}", access_lost_reinits, e);
                    }
                }
            }
            Err(e) => {
                consecutive_errors += 1;
                log::error!("캡처 오류: {}", e);
                if consecutive_errors >= 30 {
                    log::warn!("연속 오류 {}회 — DXGI 재초기화 시도", consecutive_errors);
                    consecutive_errors = 0;
                    if let Err(e) = capturer.reinitialize(Config::load_or_default().capture.monitor) {
                        log::error!("재초기화 실패: {}", e);
                    }
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }

        let elapsed = frame_start.elapsed();
        if elapsed < frame_interval {
            std::thread::sleep(frame_interval - elapsed);
        }
    }
}

/// 디바이스 링크 상태 (로그는 상태가 바뀔 때만 남긴다 — 2026-10-15 로그는 49초마다
/// 5줄씩 재연결 스팸으로 1.5일 만에 회전돼 절전·복귀 시점의 증거가 날아갔다)
#[derive(Debug, Clone, Copy, PartialEq)]
enum LinkState {
    /// 아직 연결 안 됨 / 버스에 없음
    Absent,
    /// 열거는 되지만 OUT 리포트를 NAK (hid_write 타임아웃) — 펌웨어 먹통
    Hung,
    /// 연결됐고 getDeviceInfo 응답도 받음
    Up,
    /// 연결됐지만 getDeviceInfo 무응답 — 응답 경로만 죽은 반먹통(스트리밍은 가능)
    Deaf,
}

/// 쓰기 실패 뒤 연결을 정리하고 다음 재연결 시각을 정한다.
/// 먹통이면 핸들을 놓은 뒤 USB 리셋(예약 작업)을 에피소드당 1회 자동 시도한다.
fn handle_write_failure(
    e: &dyn std::error::Error,
    conn: &mut Option<DeviceConnection>,
    link: &mut LinkState,
    next_connect_at: &mut Instant,
    usb_reset_tried: &mut bool,
) {
    use device::serial::{classify_error, WriteFailure};
    *conn = None;
    match classify_error(e) {
        WriteFailure::Hung => {
            if *link != LinkState::Hung {
                log::error!("쓰기 타임아웃 — 펌웨어 먹통(OUT NAK): {}", e);
            }
            *link = LinkState::Hung;
            if !*usb_reset_tried {
                *usb_reset_tried = true;
                usb_reset_device(false);
                *next_connect_at = Instant::now() + Duration::from_secs(6);
            } else {
                *next_connect_at = Instant::now() + Duration::from_secs(30);
            }
        }
        WriteFailure::Gone => {
            if *link != LinkState::Absent {
                log::warn!("디바이스 분리됨: {} — 재연결 대기", e);
            }
            *link = LinkState::Absent;
            *next_connect_at = Instant::now() + Duration::from_secs(3);
        }
    }
}

fn sender_loop(
    config: Config,
    running: Arc<AtomicBool>,
    config_version: Arc<AtomicU32>,
    rx: mpsc::Receiver<Vec<(u8, u8, u8)>>,
    monitor_off: Arc<AtomicBool>,
    screen_black: Arc<AtomicBool>,
    suspend_pending: Arc<AtomicBool>,
    device_released: Arc<AtomicBool>,
) {
    let mut wire_map = WireMap::from_str(&config.device.wire_map);
    let mut send_interval = Duration::from_millis(config.sync.interval_ms());
    let mut brightness = config.sync.brightness;
    let mut saturation_on = config.sync.saturation > 0.0;
    let mut light_compression = config.sync.light_compression;
    let mut current_mode = config.effect.mode.clone();
    let mut effect_cfg = config.effect.clone();
    let mut turn_off_on_sleep = config.app.turn_off_on_sleep;
    let mut turn_off_on_black = config.app.turn_off_on_black;
    let mut lamps_amount = config.device.lamps_amount;

    let mut conn: Option<DeviceConnection> = None;
    let mut link = LinkState::Absent;
    let mut next_connect_at = Instant::now();
    let mut usb_reset_tried = false;
    let mut last_hung_reminder = Instant::now();

    let mut local_version = 0u32;
    let mut send_count = 0u64;
    let mut blanked_by_screen = false;

    // 컴퓨터 리듬용 오디오 미터 (필요 시 초기화)
    let mut audio_meter: Option<audio::AudioMeter> = None;
    // 소프트웨어 효과 타이머
    let mut soft_tick: f64 = 0.0;

    while running.load(Ordering::Relaxed) {
        let send_start = Instant::now();

        // ── 절전 진입 / 세션 종료 예고: LED 끄고 핸들을 놓은 뒤 복귀까지 대기 ──
        // 디바이스는 5VSB로 PC가 꺼져도 전원이 유지돼 마지막 상태로 계속 빛난다.
        // 전환 중에 먹통이 되더라도 "꺼진 상태"로 멈추도록, 전환 전에 반드시 끈다.
        if suspend_pending.load(Ordering::SeqCst) {
            if let Some(c) = conn.take() {
                log::info!("절전/종료 전환 — LED 끄고 HID 핸들 해제");
                if let Err(e) = c.turn_off() {
                    log::warn!("전환 전 LED 끄기 실패: {}", e);
                }
                blanked_by_screen = false;
                link = LinkState::Absent;
            }
            device_released.store(true, Ordering::SeqCst);
            while rx.try_recv().is_ok() {}
            std::thread::sleep(Duration::from_millis(100));
            // 복귀 직후 USB 재열거가 끝날 시간을 준다
            next_connect_at = Instant::now() + Duration::from_secs(2);
            continue;
        }

        // ── 연결 보장 (백오프) ──
        if conn.is_none() {
            if Instant::now() < next_connect_at {
                while rx.try_recv().is_ok() {}
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            match DeviceConnection::connect(&config.device.com_port) {
                Ok(mut c) => {
                    let responded = c.init_device().unwrap_or(false);
                    if let Err(e) = c.set_brightness(brightness) { log::warn!("밝기 실패: {}", e); }
                    if responded {
                        log::info!("디바이스 연결 (MAC={:02x?})", c.mac);
                        link = LinkState::Up;
                        usb_reset_tried = false;
                    } else if !usb_reset_tried {
                        // 2026-10-15 20:03 사례: 재부팅 직후 열거는 되는데 정보 응답이 없음 →
                        // 2.5시간 뒤 완전 먹통. 핸들을 놓고 버스 리셋을 한 번 시도한다.
                        log::warn!("디바이스 연결됐으나 getDeviceInfo 무응답 — USB 리셋 1회 시도");
                        usb_reset_tried = true;
                        drop(c);
                        usb_reset_device(false);
                        next_connect_at = Instant::now() + Duration::from_secs(6);
                        continue;
                    } else {
                        log::warn!("디바이스 연결 — 정보 응답 없음(반먹통). 스트리밍만 시도");
                        link = LinkState::Deaf;
                    }
                    apply_mode(&c, &effect_cfg, lamps_amount);
                    device_released.store(false, Ordering::SeqCst);
                    blanked_by_screen = false;
                    conn = Some(c);
                }
                Err(e) => {
                    if link == LinkState::Hung {
                        if last_hung_reminder.elapsed() >= Duration::from_secs(600) {
                            last_hung_reminder = Instant::now();
                            log::error!("디바이스 먹통 지속 — USB 전원 재인가(물리 재삽입) 필요");
                        }
                    } else {
                        link = LinkState::Absent;
                        static ABSENT_LOGGED: AtomicBool = AtomicBool::new(false);
                        if !ABSENT_LOGGED.swap(true, Ordering::Relaxed) {
                            log::warn!("연결 실패: {} — 3초 간격 재시도 (이후 조용히)", e);
                        }
                    }
                    next_connect_at = Instant::now() + Duration::from_secs(3);
                    continue;
                }
            }
        }

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
                if let Some(d) = conn.as_ref() { let _ = d.set_brightness(brightness); }
            }

            lamps_amount = c.device.lamps_amount;

            turn_off_on_sleep = c.app.turn_off_on_sleep;
            turn_off_on_black = c.app.turn_off_on_black;

            if current_mode != c.effect.mode || effect_cfg_changed(&effect_cfg, &c.effect) {
                current_mode = c.effect.mode.clone();
                effect_cfg = c.effect.clone();
                if let Some(d) = conn.as_ref() {
                    if !apply_mode(d, &effect_cfg, lamps_amount) {
                        // 모드 적용 실패 → 다음 루프에서 재연결
                        log::warn!("모드 적용 실패 — 디바이스 재연결");
                        conn = None;
                        next_connect_at = Instant::now() + Duration::from_secs(1);
                        continue;
                    }
                }
            }
        }

        // 모니터 절전 또는 검정 화면 시 LED 끄기
        let should_blank = (turn_off_on_sleep && monitor_off.load(Ordering::Relaxed))
            || (turn_off_on_black && screen_black.load(Ordering::Relaxed));
        if should_blank {
            if !blanked_by_screen {
                log::info!("화면 꺼짐 감지 — LED 끄기");
                if let Some(d) = conn.as_ref() {
                    if let Err(e) = d.turn_off() {
                        handle_write_failure(e.as_ref(), &mut conn, &mut link, &mut next_connect_at, &mut usb_reset_tried);
                        continue;
                    }
                }
                blanked_by_screen = true;
            }
            while rx.try_recv().is_ok() {}
            std::thread::sleep(Duration::from_millis(200));
            continue;
        } else if blanked_by_screen {
            log::info!("화면 복귀 — LED 복원");
            blanked_by_screen = false;
            if let Some(d) = conn.as_ref() {
                let _ = d.set_brightness(brightness);
                apply_mode(d, &effect_cfg, lamps_amount);
            }
        }

        // 이 아래는 연결이 있어야 한다
        let d = match conn.as_ref() {
            Some(d) => d,
            None => continue,
        };

        if current_mode == LedMode::Sync {
            match rx.recv_timeout(Duration::from_millis(100)) {
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

                    if let Err(e) = d.set_sync_screen(&color_data) {
                        handle_write_failure(e.as_ref(), &mut conn, &mut link, &mut next_connect_at, &mut usb_reset_tried);
                        continue;
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
                if let Err(e) = d.set_computer_rhythm(effect_cfg.sound_index, vol) {
                    handle_write_failure(e.as_ref(), &mut conn, &mut link, &mut next_connect_at, &mut usb_reset_tried);
                    continue;
                }
            }
            while rx.try_recv().is_ok() {}
            std::thread::sleep(Duration::from_millis(40));
            continue;
        } else if current_mode == LedMode::Static && effect_cfg.soft_effect != SoftEffect::None {
            // 소프트웨어 동적 효과 (단색 숨쉬기/회전)
            soft_tick += 0.05;
            let speed_factor = effect_cfg.effect_speed as f64 / 50.0;
            let t = soft_tick * speed_factor;

            let result = match effect_cfg.soft_effect {
                SoftEffect::Breathe => {
                    // 사인파로 밝기 변화 (0.05 ~ 1.0)
                    let bright = ((t.sin() + 1.0) / 2.0 * 0.95 + 0.05) as f32;
                    let r = (effect_cfg.color_r as f32 * bright) as u8;
                    let g = (effect_cfg.color_g as f32 * bright) as u8;
                    let b = (effect_cfg.color_b as f32 * bright) as u8;
                    // setSectionLED(0x86)는 단발성 색 변경 명령 — 33Hz 연사 시 펌웨어 먹통 위험.
                    // Rotate와 동일하게 스트리밍 전용 SC(setSyncScreen) 경로로 전송한다.
                    let mapped = wire_map.apply(r, g, b);
                    let n = lamps_amount as usize;
                    let mut data = Vec::with_capacity(n * 5);
                    for i in 0..n {
                        let idx = (i + 1) as u8;
                        data.push(idx);
                        data.push(mapped[0]);
                        data.push(mapped[1]);
                        data.push(mapped[2]);
                        data.push(idx);
                    }
                    d.set_sync_screen(&data)
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
                    d.set_sync_screen(&data)
                }
                SoftEffect::None => Ok(()),
            };
            if let Err(e) = result {
                handle_write_failure(e.as_ref(), &mut conn, &mut link, &mut next_connect_at, &mut usb_reset_tried);
                continue;
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

    if let Some(c) = conn.take() {
        log::info!("앱 종료 — LED 끄기");
        let _ = c.turn_off();
    }
    device_released.store(true, Ordering::SeqCst);
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

/// 모드 적용. 성공 시 true, 실패 시 false 반환.
fn apply_mode(conn: &DeviceConnection, effect: &config::EffectConfig, lamps_amount: u32) -> bool {
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
    match result {
        Ok(()) => true,
        Err(e) => {
            log::warn!("모드 적용 실패: {}", e);
            false
        }
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

/// 먹통 디바이스 복구 시도: pnputil /restart-device 로 USB 디바이스 재시작.
/// 1순위: 예약 작업 `SyncRGB-USBReset`(최고 권한으로 등록돼 있어 UAC 없이 실행, 2026-10-16 메인PC 등록·검증).
/// 2순위(수동 호출만): 작업이 없으면 pnputil 을 UAC 승격으로 직접 실행.
/// PnP 재시작은 버스 리셋까지만 하고 VBUS 전원은 유지되므로,
/// 이걸로도 안 살아나는 완전 먹통이면 물리 재연결만 유효하다.
/// 인스턴스 경로의 시리얼 "0123456789"는 펌웨어 고정값이라 포트를 옮겨도 동일.
fn usb_reset_device(manual: bool) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const TASK_NAME: &str = "SyncRGB-USBReset";

    match std::process::Command::new("schtasks")
        .args(["/run", "/tn", TASK_NAME])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(out) if out.status.success() => {
            log::info!("USB 디바이스 리셋 실행 (예약 작업 {})", TASK_NAME);
            return;
        }
        Ok(out) => {
            log::warn!("예약 작업 {} 실행 실패 (exit {:?}): {}", TASK_NAME, out.status.code(),
                String::from_utf8_lossy(&out.stderr).trim());
        }
        Err(e) => log::warn!("schtasks 실행 실패: {}", e),
    }
    if !manual {
        log::warn!("자동 USB 리셋 불가 — 예약 작업 {} 을 관리자 권한으로 등록하면 UAC 없이 동작한다", TASK_NAME);
        return;
    }

    log::info!("USB 디바이스 리셋 시도 (pnputil, UAC 승인 필요)");
    let instance = format!(r"USB\VID_{:04X}&PID_{:04X}\0123456789",
        device::serial::VENDOR_ID, device::serial::PRODUCT_ID);
    let ps_cmd = format!(
        "Start-Process pnputil -Verb RunAs -WindowStyle Hidden -ArgumentList '/restart-device','{}'",
        instance);
    if let Err(e) = std::process::Command::new("powershell")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps_cmd])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
    {
        log::error!("USB 리셋 실행 실패: {}", e);
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

// ── Windows 종료·절전 감지 (WM_QUERYENDSESSION / WM_ENDSESSION / PBT_APMSUSPEND) ──
//
// 전환 전에 sender 스레드가 LED를 끄고 HID 핸들을 놓았는지를 핸드셰이크로 확인한다.
// 이전 구현은 WM_ENDSESSION에서 800ms 잠들고 끝이라, sender 가 쓰기 타임아웃(1초)에
// 걸려 있으면 끄기 명령이 나가기 전에 프로세스가 죽었다(2026-10-16 02:04 로그에 종료 기록 없음).

static SHUTDOWN_FLAG: OnceLock<Arc<AtomicBool>> = OnceLock::new();
static MONITOR_OFF_FLAG: OnceLock<Arc<AtomicBool>> = OnceLock::new();
static SUSPEND_FLAG: OnceLock<Arc<AtomicBool>> = OnceLock::new();
static RELEASED_FLAG: OnceLock<Arc<AtomicBool>> = OnceLock::new();

/// sender 에게 "LED 끄고 핸들 놓아라"를 요청하고 완료(또는 시한)까지 기다린다.
fn request_device_release(reason: &str, max_wait: Duration) {
    let (Some(suspend), Some(released)) = (SUSPEND_FLAG.get(), RELEASED_FLAG.get()) else { return; };
    log::info!("{} — LED 끄기 요청", reason);
    suspend.store(true, Ordering::SeqCst);
    let start = Instant::now();
    while !released.load(Ordering::SeqCst) && start.elapsed() < max_wait {
        std::thread::sleep(Duration::from_millis(20));
    }
    if released.load(Ordering::SeqCst) {
        log::info!("{} — LED 끄기 완료 ({}ms)", reason, start.elapsed().as_millis());
    } else {
        log::warn!("{} — LED 끄기 미완료 (시한 {}ms 초과)", reason, max_wait.as_millis());
    }
}

fn resume_device(reason: &str) {
    if let Some(suspend) = SUSPEND_FLAG.get() {
        if suspend.swap(false, Ordering::SeqCst) {
            log::info!("{} — 디바이스 재연결 예정", reason);
        }
    }
}

fn spawn_shutdown_listener(
    running: Arc<AtomicBool>,
    monitor_off: Arc<AtomicBool>,
    suspend_pending: Arc<AtomicBool>,
    device_released: Arc<AtomicBool>,
) {
    SHUTDOWN_FLAG.set(running).ok();
    MONITOR_OFF_FLAG.set(monitor_off).ok();
    SUSPEND_FLAG.set(suspend_pending).ok();
    RELEASED_FLAG.set(device_released).ok();
    std::thread::Builder::new()
        .name("shutdown".into())
        .spawn(|| unsafe {
            use windows::Win32::UI::WindowsAndMessaging::*;
            use windows::Win32::System::Power::RegisterPowerSettingNotification;
            use windows::Win32::System::Threading::SetProcessShutdownParameters;
            const SHUTDOWN_NORETRY: u32 = 0x0001; // winbase.h
            use windows::Win32::System::SystemServices::GUID_CONSOLE_DISPLAY_STATE;
            use windows::Win32::Foundation::HANDLE;

            // 종료 알림을 다른 앱보다 먼저 받아 LED 끄기 시간을 확보한다 (0x3FF = 최우선)
            let _ = SetProcessShutdownParameters(0x3FF, SHUTDOWN_NORETRY);

            let class_name = windows::core::w!("SyncRGB_Shutdown");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(shutdown_wnd_proc),
                lpszClassName: class_name,
                ..Default::default()
            };
            RegisterClassW(&wc);

            // 숨겨진 top-level 윈도우 (WM_ENDSESSION + 전원 알림 수신용)
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0), class_name,
                windows::core::w!(""), WINDOW_STYLE(0),
                0, 0, 0, 0,
                None, None, None, None,
            );

            // 모니터 전원 상태 변경 알림 등록
            if let Ok(hwnd) = hwnd {
                let _ = RegisterPowerSettingNotification(
                    HANDLE(hwnd.0 as _), &GUID_CONSOLE_DISPLAY_STATE,
                    REGISTER_NOTIFICATION_FLAGS(0),
                );
            }

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
    // WM_POWERBROADCAST wParam (winuser.h)
    const PBT_APMSUSPEND: usize = 0x0004;
    const PBT_APMRESUMESUSPEND: usize = 0x0007;
    const PBT_APMRESUMEAUTOMATIC: usize = 0x0012;
    const PBT_POWERSETTINGCHANGE: usize = 0x8013;

    match msg {
        WM_QUERYENDSESSION => {
            // 응답 시한 5초 안에 끄기를 끝낸다 (정상 디바이스 ~0.7초, 먹통이면 쓰기 타임아웃 1초+)
            request_device_release("세션 종료 예고", Duration::from_millis(3000));
            LRESULT(1)
        }
        WM_ENDSESSION => {
            if wparam.0 != 0 {
                // QUERY 를 못 받은 경로(ENDSESSION_CRITICAL 등) 대비 한 번 더
                request_device_release("세션 종료", Duration::from_millis(2000));
                if let Some(running) = SHUTDOWN_FLAG.get() {
                    running.store(false, Ordering::SeqCst);
                }
            } else {
                // 다른 앱이 종료를 막아 취소됨
                resume_device("세션 종료 취소");
            }
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            use windows::Win32::System::Power::POWERBROADCAST_SETTING;
            use windows::Win32::System::SystemServices::GUID_CONSOLE_DISPLAY_STATE;
            match wparam.0 {
                PBT_APMSUSPEND => {
                    // 앱에 주어지는 시간은 약 2초
                    request_device_release("절전 진입", Duration::from_millis(1800));
                }
                PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => {
                    resume_device("절전 복귀");
                }
                PBT_POWERSETTINGCHANGE if lparam.0 != 0 => {
                    let setting = &*(lparam.0 as *const POWERBROADCAST_SETTING);
                    if setting.PowerSetting == GUID_CONSOLE_DISPLAY_STATE {
                        let state = setting.Data[0];
                        if let Some(flag) = MONITOR_OFF_FLAG.get() {
                            // 0 = 꺼짐, 1 = 켜짐, 2 = 어두워짐
                            flag.store(state == 0, Ordering::SeqCst);
                            log::info!("모니터 전원 상태: {}", match state {
                                0 => "꺼짐", 1 => "켜짐", _ => "어두워짐"
                            });
                        }
                    }
                }
                _ => {}
            }
            LRESULT(1)
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
        tray: None, toggle_item: None, settings_item: None, reset_item: None, quit_item: None,
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
    reset_item: Option<MenuItem>,
    quit_item: Option<MenuItem>,
    locale: &'static gui::Locale,
}

impl ApplicationHandler for TrayApp {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
        if self.tray.is_some() { return; }

        let menu = Menu::new();
        let toggle = MenuItem::new(self.locale.tray_pause, true, None);
        let settings = MenuItem::new(self.locale.tray_settings, true, None);
        let reset = MenuItem::new(self.locale.tray_reset_usb, true, None);
        let quit = MenuItem::new(self.locale.tray_quit, true, None);
        menu.append(&toggle).ok();
        menu.append(&settings).ok();
        menu.append(&reset).ok();
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
        self.reset_item = Some(reset);
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
            if let Some(ref r) = self.reset_item {
                if event.id() == r.id() { usb_reset_device(true); }
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
