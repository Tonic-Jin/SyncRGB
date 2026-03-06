/// Windows WASAPI 오디오 피크 미터
/// IAudioMeterInformation으로 시스템 출력 볼륨 레벨을 읽음 (0.0 ~ 1.0)
/// 원본 SyncLight와 동일: 노이즈 게이트(≤0.01→0), 스무딩 없이 원시값 전송

use windows::Win32::Media::Audio::{
    eRender, eConsole,
    IMMDeviceEnumerator, MMDeviceEnumerator,
};
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};

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

    /// 피크 레벨 → 0~100 정수 (원본 SyncLight 동일 로직)
    /// 노이즈 게이트: ≤0.01 → 0, 그 외 raw * 100
    pub fn peak_volume(&mut self) -> u8 {
        let raw = unsafe {
            self.meter.GetPeakValue().unwrap_or(0.0)
        };

        let level = if raw <= 0.01 { 0.0 } else { raw };

        (level * 100.0) as u8
    }
}
