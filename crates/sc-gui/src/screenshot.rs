use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, SetStretchBltMode, StretchBlt, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HALFTONE, SRCCOPY,
};

pub fn capture_region(x: i32, y: i32, w: i32, h: i32, out_w: i32) -> Option<egui::ColorImage> {
    if w <= 0 || h <= 0 || out_w <= 0 {
        return None;
    }
    let out_h = ((out_w as i64 * h as i64) / w as i64).max(1) as i32;
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let mem = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, out_w, out_h);
        let previous = SelectObject(mem, bitmap.into());
        SetStretchBltMode(mem, HALFTONE);
        let copied = if w == out_w && h == out_h {
            BitBlt(mem, 0, 0, out_w, out_h, Some(screen), x, y, SRCCOPY).is_ok()
        } else {
            StretchBlt(mem, 0, 0, out_w, out_h, Some(screen), x, y, w, h, SRCCOPY).as_bool()
        };
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: out_w,
                biHeight: -out_h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; (out_w * out_h * 4) as usize];
        let lines = GetDIBits(
            mem,
            bitmap,
            0,
            out_h as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        SelectObject(mem, previous);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        if !copied || lines == 0 {
            return None;
        }
        let (chunks, _) = pixels.as_chunks_mut::<4>();
        for px in chunks {
            px.swap(0, 2);
            px[3] = 255;
        }
        Some(egui::ColorImage::from_rgba_unmultiplied(
            [out_w as usize, out_h as usize],
            &pixels,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_region_is_scaled_to_the_requested_width_keeping_its_shape() {
        if let Some(img) = capture_region(0, 0, 1920, 1080, 480) {
            assert_eq!(img.size, [480, 270]);
        }
    }

    #[test]
    fn an_empty_region_gives_nothing() {
        assert!(capture_region(0, 0, 0, 1080, 480).is_none());
        assert!(capture_region(0, 0, 1920, 1080, 0).is_none());
    }
}
