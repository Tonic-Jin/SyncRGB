/// HID 디바이스 인터페이스 정보 출력 도구
fn main() {
    let api = hidapi::HidApi::new().expect("HidApi 초기화 실패");

    println!("=== VID=0x1A86 HID 디바이스 목록 ===\n");

    for dev in api.device_list() {
        if dev.vendor_id() == 0x1A86 {
            println!("VID={:#06x} PID={:#06x}", dev.vendor_id(), dev.product_id());
            println!("  경로: {:?}", dev.path());
            println!("  제품: {:?}", dev.product_string());
            println!("  제조사: {:?}", dev.manufacturer_string());
            println!("  시리얼: {:?}", dev.serial_number());
            println!("  인터페이스: {}", dev.interface_number());
            println!("  Usage Page: {:#06x}", dev.usage_page());
            println!("  Usage: {:#06x}", dev.usage());
            println!();

            // 각 인터페이스에 연결 시도
            match api.open_path(dev.path()) {
                Ok(device) => {
                    println!("  [연결 성공]");

                    // 쓰기 테스트 (빈 리포트)
                    let mut report = vec![0u8; 65];
                    report[0] = 0x00; // Report ID
                    match device.write(&report) {
                        Ok(n) => println!("  [쓰기 성공] {} 바이트", n),
                        Err(e) => println!("  [쓰기 실패] {}", e),
                    }

                    // Feature Report 테스트
                    let mut feat = vec![0u8; 65];
                    feat[0] = 0x00;
                    match device.send_feature_report(&feat) {
                        Ok(()) => println!("  [Feature Report 전송 성공]"),
                        Err(e) => println!("  [Feature Report 전송 실패] {}", e),
                    }
                }
                Err(e) => println!("  [연결 실패] {}", e),
            }
            println!();
        }
    }
}
