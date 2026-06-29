/// 오디오 분석 모듈
///
/// # AudioMeter (레거시)
/// Windows WASAPI IAudioMeterInformation으로 시스템 출력 전체 피크 레벨 측정.
/// 노이즈 게이트(≤0.01→0), 스무딩 없이 원시값 전송.
///
/// # LoopbackAnalyzer (FFT 주파수 대역 분석)
/// WASAPI 루프백 캡처로 실제 PCM 샘플을 수집 후 FFT를 적용해
/// 지정한 주파수 대역(예: 베이스 60-250 Hz)의 에너지만 0-100으로 반환.

use windows::Win32::Media::Audio::{
    eRender, eConsole,
    IMMDeviceEnumerator, MMDeviceEnumerator,
    IAudioClient, IAudioCaptureClient,
    AUDCLNT_SHAREMODE_SHARED,
    WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
};
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use rustfft::{FftPlanner, num_complex::Complex};
use std::sync::Arc;

// ── WASAPI 플래그 상수 ─────────────────────────────────────────────────────
const AUDCLNT_STREAMFLAGS_LOOPBACK: u32 = 0x0002_0000;
// AUDCLNT_BUFFERFLAGS_SILENT = 0x00000002
const BUFFERFLAGS_SILENT: u32 = 0x0000_0002;
// wFormatTag 값
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
// IEEE float SubFormat GUID의 data1 필드 값
const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT_DATA1: u32 = 3;

// FFT 크기 (2048 샘플 ≈ 43ms @ 48kHz)
const FFT_SIZE: usize = 2048;

// 스무딩: 이전 레벨이 새 레벨보다 높으면 decay 적용
// 값이 클수록 더 오래 유지 (0.0 = 즉시, 1.0 = 유지)
const DECAY: f32 = 0.80;

// 노이즈 플로어: 이 값 이하의 정규화 레벨은 0으로 처리 (저음량 플리커 억제)
const NOISE_FLOOR: f32 = 0.08;

// 파워 커브: 1.0 = 선형, 값이 클수록 저음량 반응이 줄어들고 고음량 반응이 강조됨
const POWER_CURVE: f32 = 1.6;

// ── AudioMeter (레거시) ────────────────────────────────────────────────────

pub struct AudioMeter {
    meter: IAudioMeterInformation,
}

impl AudioMeter {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
            let meter: IAudioMeterInformation = device.Activate(CLSCTX_ALL, None)?;

            Ok(Self { meter })
        }
    }

    /// 피크 레벨 → 0~100 정수
    /// 노이즈 게이트: ≤0.01 → 0
    pub fn peak_volume(&mut self) -> u8 {
        let raw = unsafe {
            self.meter.GetPeakValue().unwrap_or(0.0)
        };
        let level = if raw <= 0.01 { 0.0 } else { raw };
        (level * 100.0) as u8
    }
}

// ── LoopbackAnalyzer (FFT 주파수 대역) ────────────────────────────────────

pub struct LoopbackAnalyzer {
    _client: IAudioClient,           // 수명 유지용
    capture: IAudioCaptureClient,
    sample_rate: u32,
    channels: u16,
    /// 수집된 모노 샘플 버퍼
    sample_buf: Vec<f32>,
    /// FFT 플랜
    fft: Arc<dyn rustfft::Fft<f32>>,
    /// 직전 레벨 (decay 스무딩용)
    last_level: f32,
}

impl LoopbackAnalyzer {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;

            // 엔드포인트 기본 포맷 조회 (반드시 그대로 써야 함)
            // WAVEFORMATEX는 1-byte packed 구조체이므로 read_unaligned 사용
            let fmt_ptr: *mut WAVEFORMATEX = client.GetMixFormat()?;
            let format_tag = std::ptr::addr_of!((*fmt_ptr).wFormatTag).read_unaligned();
            let sample_rate = std::ptr::addr_of!((*fmt_ptr).nSamplesPerSec).read_unaligned();
            let channels = std::ptr::addr_of!((*fmt_ptr).nChannels).read_unaligned();

            // IEEE float 포맷 여부 확인
            let is_float = format_tag == WAVE_FORMAT_IEEE_FLOAT
                || (format_tag == WAVE_FORMAT_EXTENSIBLE && {
                    let ext_ptr = fmt_ptr as *const WAVEFORMATEXTENSIBLE;
                    let sub_data1 = std::ptr::addr_of!((*ext_ptr).SubFormat.data1).read_unaligned();
                    sub_data1 == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT_DATA1
                });

            if !is_float {
                CoTaskMemFree(Some(fmt_ptr as *mut _));
                return Err(format!(
                    "지원되지 않는 오디오 포맷 (wFormatTag={}). IEEE float 필요",
                    format_tag
                ).into());
            }

            // 루프백 스트림 초기화 (200ms 버퍼)
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK,
                2_000_000, // 200ms (100ns 단위)
                0,
                fmt_ptr,
                None,
            )?;

            CoTaskMemFree(Some(fmt_ptr as *mut _));

            let capture: IAudioCaptureClient = client.GetService()?;
            client.Start()?;

            let mut planner = FftPlanner::<f32>::new();
            let fft = planner.plan_fft_forward(FFT_SIZE);

            log::info!(
                "LoopbackAnalyzer 초기화: {}Hz, {}ch",
                sample_rate, channels
            );

            Ok(Self {
                _client: client,
                capture,
                sample_rate,
                channels,
                sample_buf: Vec::with_capacity(FFT_SIZE * 4),
                fft,
                last_level: 0.0,
            })
        }
    }

    /// 지정한 주파수 대역의 에너지를 0-100으로 반환.
    ///
    /// - `low_hz`: 하한 주파수 (Hz)
    /// - `high_hz`: 상한 주파수 (Hz)
    pub fn band_level(&mut self, low_hz: f32, high_hz: f32) -> u8 {
        // 1. 버퍼에서 사용 가능한 오디오 패킷 수집
        unsafe { self.drain_capture_buffer(); }

        // 샘플이 FFT_SIZE에 미달이면 직전 값 유지
        if self.sample_buf.len() < FFT_SIZE {
            return (self.last_level * 100.0) as u8;
        }

        // 2. 가장 최근 FFT_SIZE 샘플에 Hann 윈도우 적용
        let start = self.sample_buf.len() - FFT_SIZE;
        let mut fft_buf: Vec<Complex<f32>> = self.sample_buf[start..]
            .iter()
            .enumerate()
            .map(|(i, &x)| {
                let w = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32
                    / (FFT_SIZE - 1) as f32).cos());
                Complex::new(x * w, 0.0)
            })
            .collect();

        self.fft.process(&mut fft_buf);

        // 3. 대상 주파수 bin 인덱스 계산
        let bin_hz = self.sample_rate as f32 / FFT_SIZE as f32;
        let bin_start = ((low_hz  / bin_hz).floor() as usize).max(1).min(FFT_SIZE / 2 - 1);
        let bin_end   = ((high_hz / bin_hz).ceil()  as usize).min(FFT_SIZE / 2);
        let num_bins  = (bin_end - bin_start).max(1) as f32;

        // 4. 평균 크기(magnitude) 계산
        let avg_mag = fft_buf[bin_start..=bin_end.min(FFT_SIZE / 2)]
            .iter()
            .map(|c| c.norm())
            .sum::<f32>() / num_bins;

        // 5. 정규화
        // Hann 윈도우 + 풀스케일 단일 주파수: 크기 ≈ FFT_SIZE / 4
        // 일반 음악은 풀스케일에 훨씬 못 미치므로 ×4 보정
        let normalized = (avg_mag / (FFT_SIZE as f32 / 4.0) * 4.0).min(1.0);

        // 6. 노이즈 플로어 + 파워 커브 + decay 스무딩
        // 노이즈 플로어 이하: 완전 소음으로 처리 (저음량 플리커 억제)
        // 노이즈 플로어 초과: 0-1 범위로 재매핑 후 파워 커브 적용
        let target = if normalized <= NOISE_FLOOR {
            0.0
        } else {
            let remapped = (normalized - NOISE_FLOOR) / (1.0 - NOISE_FLOOR);
            remapped.powf(POWER_CURVE)
        };
        if target >= self.last_level {
            self.last_level = target;          // 즉시 상승
        } else {
            self.last_level = self.last_level * DECAY + target * (1.0 - DECAY); // 서서히 하강
        }

        // 7. 오래된 샘플 정리 (메모리 무제한 증가 방지)
        if self.sample_buf.len() > FFT_SIZE * 8 {
            let trim = self.sample_buf.len() - FFT_SIZE * 2;
            self.sample_buf.drain(0..trim);
        }

        (self.last_level * 100.0) as u8
    }

    /// WASAPI 캡처 클라이언트에서 사용 가능한 모든 프레임을 모노 f32로 변환해 버퍼에 추가.
    unsafe fn drain_capture_buffer(&mut self) {
        loop {
            let packet_size = match self.capture.GetNextPacketSize() {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            if packet_size == 0 { break; }

            let mut data_ptr: *mut u8 = std::ptr::null_mut();
            let mut frames: u32 = 0;
            let mut flags: u32 = 0;

            if self.capture.GetBuffer(
                &mut data_ptr, &mut frames, &mut flags, None, None,
            ).is_err() {
                break;
            }

            let is_silent = (flags & BUFFERFLAGS_SILENT) != 0;

            if frames > 0 {
                if is_silent {
                    // 무음 프레임: 0 추가
                    for _ in 0..frames {
                        self.sample_buf.push(0.0);
                    }
                } else {
                    let total_samples = frames as usize * self.channels as usize;
                    let samples = std::slice::from_raw_parts(
                        data_ptr as *const f32,
                        total_samples,
                    );
                    // 다채널 → 모노 다운믹스
                    let ch = self.channels as usize;
                    for chunk in samples.chunks_exact(ch) {
                        let mono = chunk.iter().sum::<f32>() / ch as f32;
                        self.sample_buf.push(mono);
                    }
                }
            }

            let _ = self.capture.ReleaseBuffer(frames);
        }
    }
}
