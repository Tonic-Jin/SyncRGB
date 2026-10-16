use hidapi::{HidApi, HidDevice};
use std::time::Duration;

use super::protocol;

/// Robobloq LED 디바이스 (USB HID)
pub const VENDOR_ID: u16 = 0x1A86;
pub const PRODUCT_ID: u16 = 0xFE07;

/// 쓰기 후 대기 시간 (ms) — SyncLight 원본: 200ms
const WRITE_DELAY_MS: u64 = 200;

/// 청크(HID 리포트) 간 최소 간격 (µs).
/// 원본 SyncLight(Electron)은 JS 이벤트 루프 오버헤드로 리포트 사이에 자연스러운
/// 간격이 있었지만 Rust는 back-to-back 전송이라 디바이스 MCU(CH55x급) 수신 버퍼가
/// 따라오지 못해 펌웨어가 먹통(USB 재삽입 전까지 무응답)이 될 수 있다.
/// thread::sleep은 Windows 타이머 해상도(최대 ~15ms) 문제로 spin-wait 사용.
const INTER_CHUNK_GAP_US: u64 = 500;

fn inter_chunk_pause() {
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_micros(INTER_CHUNK_GAP_US) {
        std::hint::spin_loop();
    }
}

/// 쓰기 실패의 종류. 로그(2026-10-15)에서 확인한 두 서명:
/// - 먹통: `hid_write/WaitForSingleObject: (0x3E5)` — 디바이스가 USB 열거는 유지한 채
///   OUT 리포트를 영원히 NAK → hidapi 1초 타임아웃. 재연결해도 똑같이 실패한다.
/// - 분리: `WriteFile: (0x48F) 장치가 연결되지 않았습니다` / `(0x1F)` — 버스에서 빠짐.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WriteFailure {
    Hung,
    Gone,
}

pub fn classify_error(e: &dyn std::error::Error) -> WriteFailure {
    let msg = e.to_string();
    if msg.contains("WaitForSingleObject") || msg.contains("0x000003E5") {
        WriteFailure::Hung
    } else {
        WriteFailure::Gone
    }
}

/// HID 연결 관리자
pub struct DeviceConnection {
    device: HidDevice,
    pub mac: [u8; 6],
}

impl DeviceConnection {
    /// HID 디바이스 연결 (interface 0).
    /// 전체 HID 열거(`HidApi::new`)는 모든 장치를 열어 문자열을 읽기 때문에 먹통 장치가
    /// 꽂혀 있으면 수십 초 걸렸다(로그: 재연결 1회 46초). 우리 VID/PID만 열거한다.
    #[allow(deprecated)] // 앱 코드에서 열거 범위를 직접 제한하는 용도 — hidapi 2.6 안내와 일치
    pub fn connect(_com_port: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let api = HidApi::new()?;

        let dev_info = api.device_list()
            .find(|d| d.vendor_id() == VENDOR_ID && d.product_id() == PRODUCT_ID && d.interface_number() == 0)
            .ok_or_else(|| format!("Robobloq HID 디바이스를 찾을 수 없음 (VID={:#06x}, PID={:#06x})",
                VENDOR_ID, PRODUCT_ID))?;

        let path = dev_info.path().to_owned();
        let device = api.open_path(&path)?;
        device.set_blocking_mode(false)?;

        log::info!("HID 디바이스 연결: VID={:#06x}, PID={:#06x} (interface 0)",
            VENDOR_ID, PRODUCT_ID);

        let conn = Self { device, mac: [0u8; 6] };
        conn.drain_input();
        Ok(conn)
    }

    /// 열기 직전까지 디바이스 IN 큐에 남아 있던 응답(이전 프로세스가 읽지 않은 것)을 버린다.
    /// 남은 개수는 먹통 원인 추적용 증거라 로그에 남긴다.
    fn drain_input(&self) {
        let mut buf = [0u8; 256];
        let mut n_stale = 0u32;
        while let Ok(n) = self.device.read_timeout(&mut buf, 0) {
            if n == 0 { break; }
            n_stale += 1;
            if n_stale >= 64 { break; }
        }
        if n_stale > 0 {
            log::warn!("연결 시 읽지 않은 응답 {}건 폐기 (이전 세션이 남긴 IN 리포트)", n_stale);
        }
    }

    /// HID Output Report로 전송 (Report ID 0x00 + 데이터)
    fn hid_write(&self, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let mut report = Vec::with_capacity(data.len() + 1);
        report.push(0x00);
        report.extend_from_slice(data);
        self.device.write(&report)?;
        Ok(())
    }

    /// 패킷 전송 (64바이트 청킹) — writeWithoutResponse
    pub fn write_without_response(&self, packet: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        for (i, chunk) in protocol::chunk_packet(packet).iter().enumerate() {
            if i > 0 { inter_chunk_pause(); }
            self.hid_write(chunk)?;
        }
        Ok(())
    }

    /// 패킷 전송 + 응답 대기 — write (200ms 딜레이)
    fn write_with_response(&self, packet: &[u8]) -> Result<Option<protocol::RbResponse>, Box<dyn std::error::Error>> {
        for (i, chunk) in protocol::chunk_packet(packet).iter().enumerate() {
            if i > 0 { inter_chunk_pause(); }
            self.hid_write(chunk)?;
        }

        std::thread::sleep(Duration::from_millis(WRITE_DELAY_MS));

        let mut buf = [0u8; 256];
        match self.device.read_timeout(&mut buf, 500) {
            Ok(n) if n > 0 => Ok(protocol::parse_response(&buf[..n])),
            Ok(_) => Ok(None),
            Err(e) => {
                log::warn!("HID 읽기 실패: {}", e);
                Ok(None)
            }
        }
    }

    /// 디바이스 초기화 (MAC 획득). 응답을 받았으면 true.
    /// false = 열거는 되지만 응답 경로가 죽은 "반먹통" (스트리밍은 될 수 있음, 로그 2026-10-15 20:03).
    pub fn init_device(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        let packet = protocol::get_device_info();
        let resp = self.write_with_response(&packet)?;

        let responded = match resp {
            Some(r) if r.payload.len() >= 7 => {
                self.mac.copy_from_slice(&r.payload[1..7]);
                log::info!("디바이스 MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    self.mac[0], self.mac[1], self.mac[2],
                    self.mac[3], self.mac[4], self.mac[5]);
                true
            }
            Some(_) => true,
            None => false,
        };

        std::thread::sleep(Duration::from_millis(80));
        Ok(responded)
    }

    // ── 화면 동기화 ──

    pub fn set_sync_screen(&self, colors: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let packet = protocol::set_sync_screen(colors);
        self.write_without_response(&packet)
    }

    // ── 기본 제어 (dedicated action codes) ──

    pub fn set_brightness(&self, val: u8) -> Result<(), Box<dyn std::error::Error>> {
        let packet = protocol::set_brightness(val);
        self.write_with_response(&packet)?;
        Ok(())
    }

    pub fn turn_off(&self) -> Result<(), Box<dyn std::error::Error>> {
        let packet = protocol::turn_off_light();
        self.write_with_response(&packet)?;
        Ok(())
    }

    // ── LED 이펙트 (dedicated action codes) ──

    /// LED 효과 설정 (effectType: 2=동적, 3=음악반응)
    pub fn set_led_effect(&self, effect_type: u8, effect_index: u8) -> Result<(), Box<dyn std::error::Error>> {
        let packet = protocol::set_led_effect(effect_type, effect_index);
        self.write_with_response(&packet)?;
        Ok(())
    }

    /// LED 단색 설정 (setSectionLED)
    pub fn set_section_led(&self, r: u8, g: u8, b: u8, lamps_amount: u32) -> Result<(), Box<dyn std::error::Error>> {
        let packet = protocol::set_section_led(r, g, b, lamps_amount);
        self.write_without_response(&packet)
    }

    pub fn set_dynamic_speed(&self, speed: u8) -> Result<(), Box<dyn std::error::Error>> {
        let packet = protocol::set_dynamic_speed(speed);
        self.write_with_response(&packet)?;
        Ok(())
    }

    /// 컴퓨터 리듬 전송 (effectIndex + volume 0-100) — 응답 대기 없이 빠르게
    pub fn set_computer_rhythm(&self, effect_index: u8, volume: u8) -> Result<(), Box<dyn std::error::Error>> {
        let packet = protocol::set_computer_rhythm(effect_index, volume);
        self.write_without_response(&packet)
    }
}
