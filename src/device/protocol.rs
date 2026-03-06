/// RB/SC 프로토콜 패킷 빌더
///
/// RB 패킷: "RB"(2) + 길이(1) + 세션ID(1) + 액션(1) + 페이로드 + 체크섬(1)
/// SC 패킷: "SC"(2) + 길이(2, BigEndian) + 세션ID(1) + 액션(1) + 데이터 + 체크섬(1)
///
/// USB HID 디바이스는 dedicated action code 프로토콜 사용 (MAC 없음, 짧은 패킷)
/// BLE/Dongle은 notSyncEffect 래퍼 사용 (20바이트, MAC 포함) — 여기서는 USB HID만 지원

const HEADER_RB: &[u8] = b"RB";
const MAX_CHUNK_SIZE: usize = 64;

/// 액션 코드 (USB HID dedicated protocol)
mod action {
    pub const SYNC_SCREEN: u8 = 128;      // 0x80
    pub const READ_DEVICE_INFO: u8 = 130;  // 0x82
    pub const SET_LED_EFFECT: u8 = 133;    // 0x85
    pub const SET_SECTION_LED: u8 = 134;   // 0x86
    pub const SET_BRIGHTNESS: u8 = 135;    // 0x87
    pub const SET_DYNAMIC_SPEED: u8 = 138;     // 0x8A
    pub const TURN_OFF_LIGHT: u8 = 151;        // 0x97
    pub const SET_COMPUTER_RHYTHM: u8 = 152;   // 0x98
}

/// wireMap: LED 디바이스의 색상 채널 순서
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WireMap {
    RGB,
    RBG,
    GRB,
    GBR,
    BRG,
    BGR,
}

impl WireMap {
    pub fn from_str(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "RBG" => Self::RBG,
            "GRB" => Self::GRB,
            "GBR" => Self::GBR,
            "BRG" => Self::BRG,
            "BGR" => Self::BGR,
            _ => Self::RGB,
        }
    }

    pub fn apply(&self, r: u8, g: u8, b: u8) -> [u8; 3] {
        match self {
            Self::RGB => [r, g, b],
            Self::RBG => [r, b, g],
            Self::GRB => [g, r, b],
            Self::GBR => [g, b, r],
            Self::BRG => [b, r, g],
            Self::BGR => [b, g, r],
        }
    }
}

/// 체크섬: 모든 바이트 합산 (wrapping)
fn checksum(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |acc, &b| acc.wrapping_add(b))
}

/// 세션 ID 카운터 (글로벌, 1~255 순환)
static SESSION_COUNTER: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(1);

fn next_session_id() -> u8 {
    let id = SESSION_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if id == 0 { SESSION_COUNTER.store(1, std::sync::atomic::Ordering::Relaxed); }
    id
}

/// RB 패킷 생성 (dedicated action code)
fn build_rb_packet(action: u8, payload: &[u8]) -> Vec<u8> {
    let sid = next_session_id();
    let total_len = 2 + 1 + 1 + 1 + payload.len() + 1;
    let mut packet = Vec::with_capacity(total_len);

    packet.extend_from_slice(HEADER_RB);
    packet.push(total_len as u8);
    packet.push(sid);
    packet.push(action);
    packet.extend_from_slice(payload);
    let chk = checksum(&packet);
    packet.push(chk);

    packet
}

// ── 디바이스 정보 ──

/// 디바이스 정보 요청 (readDeviceInfo = 0x82)
pub fn get_device_info() -> Vec<u8> {
    build_rb_packet(action::READ_DEVICE_INFO, &[])
}

// ── 화면 동기화 (SC 프로토콜) ──

/// 화면 동기화 색상 전송 (setSyncScreen = 0x80)
/// SC 프로토콜: "SC"(2) + 길이(2, BigEndian) + 세션ID(1) + 액션(1) + 색상데이터 + 체크섬(1)
pub fn set_sync_screen(colors: &[u8]) -> Vec<u8> {
    let sid = next_session_id();
    let total_len = 7 + colors.len();
    let mut packet = Vec::with_capacity(total_len);

    packet.push(0x53); // 'S'
    packet.push(0x43); // 'C'
    packet.push(((total_len >> 8) & 0xFF) as u8);
    packet.push((total_len & 0xFF) as u8);
    packet.push(sid);
    packet.push(action::SYNC_SCREEN);
    packet.extend_from_slice(colors);
    let chk = checksum(&packet);
    packet.push(chk);

    packet
}

// ── LED 이펙트 (dedicated action codes, MAC 불필요) ──

/// LED 효과 설정 (setLedEffect = 0x85)
/// 8바이트: "RB" + 8 + sid + 133 + effectType + effectIndex + checksum
/// effectType: 2=동적, 3=음악반응
pub fn set_led_effect(effect_type: u8, effect_index: u8) -> Vec<u8> {
    build_rb_packet(action::SET_LED_EFFECT, &[effect_type, effect_index])
}

/// LED 단색 설정 (setSectionLED = 0x86)
/// 가변: "RB" + len + sid + 134 + data... + checksum
/// data: [1, R, G, B, 254] (기본) 또는 [1, R, G, B, lampsAmount, lampsAmount+1, 0, 0, 0, 254]
pub fn set_section_led(r: u8, g: u8, b: u8, lamps_amount: u32) -> Vec<u8> {
    let data = if lamps_amount > 0 && lamps_amount < 254 {
        let la = lamps_amount as u8;
        vec![1, r, g, b, la, la + 1, 0, 0, 0, 254]
    } else {
        vec![1, r, g, b, 254]
    };
    build_rb_packet(action::SET_SECTION_LED, &data)
}

/// 밝기 설정 (setBrightness = 0x87)
/// 7바이트: "RB" + 7 + sid + 135 + value + checksum
pub fn set_brightness(val: u8) -> Vec<u8> {
    build_rb_packet(action::SET_BRIGHTNESS, &[val.max(5)])
}

/// 동적 효과 속도 설정 (setDynamicSpeed = 0x8A)
/// 7바이트: "RB" + 7 + sid + 138 + speed + checksum
pub fn set_dynamic_speed(speed: u8) -> Vec<u8> {
    build_rb_packet(action::SET_DYNAMIC_SPEED, &[speed.clamp(5, 100)])
}

/// LED 끄기 (turnOffLight = 0x97)
/// 6바이트: "RB" + 6 + sid + 151 + checksum
pub fn turn_off_light() -> Vec<u8> {
    build_rb_packet(action::TURN_OFF_LIGHT, &[])
}

/// 컴퓨터 리듬 (setComputerRhythm = 0x98)
/// 8바이트: "RB" + 8 + sid + 152 + effectIndex + volume(0-100) + checksum
pub fn set_computer_rhythm(effect_index: u8, volume: u8) -> Vec<u8> {
    build_rb_packet(action::SET_COMPUTER_RHYTHM, &[effect_index, volume.min(100)])
}

// ── 패킷 유틸리티 ──

/// 패킷을 64바이트 청크로 분할
pub fn chunk_packet(packet: &[u8]) -> Vec<Vec<u8>> {
    if packet.len() <= MAX_CHUNK_SIZE {
        return vec![packet.to_vec()];
    }
    packet.chunks(MAX_CHUNK_SIZE)
        .map(|c| c.to_vec())
        .collect()
}

/// 응답 패킷 파싱 결과
#[derive(Debug)]
pub struct RbResponse {
    pub payload: Vec<u8>,
}

/// 응답 패킷 파싱
pub fn parse_response(data: &[u8]) -> Option<RbResponse> {
    if data.len() < 6 || &data[0..2] != HEADER_RB {
        return None;
    }

    let total_len = data[2] as usize;
    if data.len() < total_len {
        return None;
    }

    let payload = data[5..total_len - 1].to_vec();
    Some(RbResponse { payload })
}
