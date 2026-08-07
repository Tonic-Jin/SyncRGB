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

/// HID 연결 관리자
pub struct DeviceConnection {
    device: HidDevice,
    pub mac: [u8; 6],
}

impl DeviceConnection {
    /// HID 디바이스 연결 (interface 0)
    pub fn connect(_com_port: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let api = HidApi::new()?;

        let dev_info = api.device_list()
            .find(|d| {
                d.vendor_id() == VENDOR_ID
                    && d.product_id() == PRODUCT_ID
                    && d.interface_number() == 0
            })
            .ok_or_else(|| {
                // 목록 덤프는 프로세스당 1회만 — 3초 재시도마다 찍으면 파일 로그가 폭주한다
                static LISTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
                if !LISTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    log::warn!("HID 디바이스 목록:");
                    for dev in api.device_list() {
                        log::warn!("  VID={:#06x} PID={:#06x} page={:#06x} if={} {:?}",
                            dev.vendor_id(), dev.product_id(),
                            dev.usage_page(), dev.interface_number(),
                            dev.product_string().unwrap_or_default());
                    }
                }
                format!("Robobloq HID 디바이스를 찾을 수 없음 (VID={:#06x}, PID={:#06x})",
                    VENDOR_ID, PRODUCT_ID)
            })?;

        let path = dev_info.path().to_owned();
        let device = api.open_path(&path)?;
        device.set_blocking_mode(false)?;

        log::info!("HID 디바이스 연결: VID={:#06x}, PID={:#06x} (interface 0)",
            VENDOR_ID, PRODUCT_ID);

        Ok(Self {
            device,
            mac: [0u8; 6],
        })
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

    /// 디바이스 초기화 (MAC 획득)
    pub fn init_device(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let packet = protocol::get_device_info();
        let resp = self.write_with_response(&packet)?;

        if let Some(r) = resp {
            if r.payload.len() >= 7 {
                self.mac.copy_from_slice(&r.payload[1..7]);
                log::info!("디바이스 MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    self.mac[0], self.mac[1], self.mac[2],
                    self.mac[3], self.mac[4], self.mac[5]);
            }
        } else {
            log::warn!("getDeviceInfo 응답 없음 — 디바이스가 먹통 상태일 수 있음 (USB 재연결 필요 가능성)");
        }

        std::thread::sleep(Duration::from_millis(80));
        Ok(())
    }

    /// 디바이스 생존 확인 — getDeviceInfo에 응답이 오면 true.
    /// 먹통(펌웨어 hang) 상태면 enumeration은 살아 있어도 응답이 없다.
    pub fn probe(&self) -> bool {
        let packet = protocol::get_device_info();
        matches!(self.write_with_response(&packet), Ok(Some(_)))
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
