fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let ico_path = format!("{}/app.ico", out_dir);

    let sizes: &[u32] = &[16, 32, 48, 256];
    std::fs::write(&ico_path, generate_ico(sizes)).expect("ICO 생성 실패");

    let mut res = winresource::WindowsResource::new();
    res.set_icon(&ico_path);
    res.set("ProductName", "SyncRGB");
    res.set("FileDescription", "SyncRGB LED 컨트롤러");
    res.compile().expect("리소스 컴파일 실패");
}

fn generate_ico(sizes: &[u32]) -> Vec<u8> {
    let mut data = Vec::new();

    // ICONDIR header
    data.extend_from_slice(&0u16.to_le_bytes()); // reserved
    data.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    data.extend_from_slice(&(sizes.len() as u16).to_le_bytes());

    let mut bmp_blocks: Vec<Vec<u8>> = Vec::new();
    for &size in sizes {
        bmp_blocks.push(rgba_to_bmp(&generate_spectrum_ring(size), size));
    }

    // Directory entries
    let header_size = 6 + sizes.len() * 16;
    let mut offset = header_size;
    for (i, &size) in sizes.iter().enumerate() {
        let w = if size >= 256 { 0u8 } else { size as u8 };
        data.push(w);
        data.push(w);
        data.push(0); // color count
        data.push(0); // reserved
        data.extend_from_slice(&1u16.to_le_bytes()); // color planes
        data.extend_from_slice(&32u16.to_le_bytes()); // bpp
        data.extend_from_slice(&(bmp_blocks[i].len() as u32).to_le_bytes());
        data.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += bmp_blocks[i].len();
    }

    for block in &bmp_blocks {
        data.extend_from_slice(block);
    }
    data
}

fn rgba_to_bmp(rgba: &[u8], size: u32) -> Vec<u8> {
    let mut data = Vec::new();

    // BITMAPINFOHEADER (40 bytes)
    data.extend_from_slice(&40u32.to_le_bytes());
    data.extend_from_slice(&(size as i32).to_le_bytes());
    data.extend_from_slice(&((size * 2) as i32).to_le_bytes()); // doubled for ICO
    data.extend_from_slice(&1u16.to_le_bytes()); // planes
    data.extend_from_slice(&32u16.to_le_bytes()); // bpp
    data.extend_from_slice(&0u32.to_le_bytes()); // compression
    data.extend_from_slice(&0u32.to_le_bytes()); // image size
    data.extend_from_slice(&0i32.to_le_bytes()); // x ppm
    data.extend_from_slice(&0i32.to_le_bytes()); // y ppm
    data.extend_from_slice(&0u32.to_le_bytes()); // colors used
    data.extend_from_slice(&0u32.to_le_bytes()); // colors important

    // Pixel data: BGRA, bottom-to-top
    for y in (0..size).rev() {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            data.push(rgba[idx + 2]); // B
            data.push(rgba[idx + 1]); // G
            data.push(rgba[idx]);     // R
            data.push(rgba[idx + 3]); // A
        }
    }

    // AND mask (all zeros — alpha handles transparency)
    let row_bytes = ((size + 31) / 32) * 4;
    data.extend(std::iter::repeat(0u8).take((row_bytes * size) as usize));
    data
}

fn generate_spectrum_ring(size: u32) -> Vec<u8> {
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

            let outer_mask = smoothstep(outer_r + aa, outer_r - aa, dist);
            let inner_mask = smoothstep(inner_r - aa, inner_r + aa, dist);
            let alpha = outer_mask * inner_mask;

            if alpha > 0.001 {
                let angle = dx.atan2(-dy);
                let hue = (angle.to_degrees() + 360.0) % 360.0;
                let (r, g, b) = hsl_to_rgb(hue, 1.0, 0.5);
                rgba[idx] = r;
                rgba[idx + 1] = g;
                rgba[idx + 2] = b;
                rgba[idx + 3] = (alpha * 255.0) as u8;
            }
        }
    }
    rgba
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
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
