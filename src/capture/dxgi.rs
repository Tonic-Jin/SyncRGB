use windows::core::Interface;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;

/// DXGI Desktop Duplication을 통한 화면 캡처
pub struct ScreenCapture {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    staging: ID3D11Texture2D,
    pub width: u32,
    pub height: u32,
}

impl ScreenCapture {
    /// 지정된 모니터의 Desktop Duplication 초기화
    pub fn new(monitor_index: u32) -> Result<Self, Box<dyn std::error::Error>> {
        unsafe {
            // D3D11 디바이스 생성
            let mut device = None;
            let mut context = None;

            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;

            let device = device.ok_or("D3D11 디바이스 생성 실패")?;
            let context = context.ok_or("D3D11 컨텍스트 생성 실패")?;

            // DXGI 어댑터 → 출력(모니터) 가져오기
            let dxgi_device: IDXGIDevice = device.cast()?;
            let adapter: IDXGIAdapter = dxgi_device.GetParent()?;
            let output: IDXGIOutput = adapter.EnumOutputs(monitor_index)?;
            let output1: IDXGIOutput1 = output.cast()?;

            // Desktop Duplication 생성
            let duplication = output1.DuplicateOutput(&device)?;

            // 출력 해상도 가져오기
            let desc = output.GetDesc()?;
            let width = (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left) as u32;
            let height = (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top) as u32;

            log::info!("화면 캡처 초기화: {}x{} (모니터 #{})", width, height, monitor_index);

            // Staging 텍스처 (GPU → CPU 복사용)
            let staging_desc = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };

            let mut staging = None;
            device.CreateTexture2D(&staging_desc, None, Some(&mut staging))?;
            let staging = staging.ok_or("Staging 텍스처 생성 실패")?;

            Ok(Self {
                device,
                context,
                duplication,
                staging,
                width,
                height,
            })
        }
    }

    /// 프레임 캡처 → BGRA 픽셀 데이터 반환
    /// 반환: (데이터, 행 피치)
    pub fn capture_frame(&mut self) -> Result<(Vec<u8>, u32), CaptureError> {
        unsafe {
            let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource = None;

            // 다음 프레임 획득 (최대 100ms 대기)
            let result = self.duplication.AcquireNextFrame(100, &mut frame_info, &mut resource);

            match result {
                Ok(()) => {}
                Err(e) => {
                    let code = e.code();
                    if code == DXGI_ERROR_WAIT_TIMEOUT {
                        return Err(CaptureError::Timeout);
                    }
                    if code == DXGI_ERROR_ACCESS_LOST {
                        return Err(CaptureError::AccessLost);
                    }
                    return Err(CaptureError::Other(format!("AcquireNextFrame 실패: {}", e)));
                }
            }

            let resource = resource.ok_or(CaptureError::Other("리소스 없음".into()))?;
            let texture: ID3D11Texture2D = resource.cast()
                .map_err(|e: windows::core::Error| CaptureError::Other(format!("{}", e)))?;

            // GPU 텍스처 → Staging 텍스처 복사
            self.context.CopyResource(&self.staging, &texture);

            // CPU에서 읽기
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context.Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|e| CaptureError::Other(format!("Map 실패: {}", e)))?;

            let pitch = mapped.RowPitch;
            let data_size = (pitch * self.height) as usize;
            let src = std::slice::from_raw_parts(mapped.pData as *const u8, data_size);
            let data = src.to_vec();

            self.context.Unmap(&self.staging, 0);
            let _ = self.duplication.ReleaseFrame();

            Ok((data, pitch))
        }
    }

    /// Desktop Duplication 재초기화 (AccessLost 시)
    pub fn reinitialize(&mut self, monitor_index: u32) -> Result<(), Box<dyn std::error::Error>> {
        unsafe {
            let dxgi_device: IDXGIDevice = self.device.cast()?;
            let adapter: IDXGIAdapter = dxgi_device.GetParent()?;
            let output: IDXGIOutput = adapter.EnumOutputs(monitor_index)?;
            let output1: IDXGIOutput1 = output.cast()?;
            self.duplication = output1.DuplicateOutput(&self.device)?;
            log::info!("Desktop Duplication 재초기화 완료");
            Ok(())
        }
    }
}

#[derive(Debug)]
pub enum CaptureError {
    Timeout,
    AccessLost,
    Other(String),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => write!(f, "프레임 타임아웃"),
            Self::AccessLost => write!(f, "Desktop Duplication 접근 손실"),
            Self::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for CaptureError {}
