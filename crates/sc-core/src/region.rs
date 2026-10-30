pub fn crop_gray(
    bgra: &[u8],
    stride: usize,
    width: u32,
    height: u32,
    region: [f32; 4],
) -> Option<(u32, u32, Vec<u8>)> {
    let clamp = |v: f32| v.clamp(0.0, 1.0);
    let [x0, y0, x1, y1] = region.map(clamp);
    let left = (x0 * width as f32).round() as usize;
    let top = (y0 * height as f32).round() as usize;
    let right = ((x1 * width as f32).round() as usize).min(width as usize);
    let bottom = ((y1 * height as f32).round() as usize).min(height as usize);
    if right <= left || bottom <= top || stride < width as usize * 4 {
        return None;
    }
    if bgra.len() < stride * (bottom - 1) + right * 4 {
        return None;
    }
    let (w, h) = (right - left, bottom - top);
    let mut gray = Vec::with_capacity(w * h);
    for row in top..bottom {
        let line = &bgra[row * stride + left * 4..row * stride + right * 4];
        for px in line.as_chunks::<4>().0 {
            let luma = (px[2] as u32 * 299 + px[1] as u32 * 587 + px[0] as u32 * 114) / 1000;
            gray.push(luma as u8);
        }
    }
    Some((w as u32, h as u32, gray))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(w: u32, h: u32, stride: usize) -> Vec<u8> {
        let mut data = vec![0u8; stride * h as usize];
        for y in 0..h as usize {
            for x in 0..w as usize {
                let i = y * stride + x * 4;
                data[i] = x as u8;
                data[i + 1] = x as u8;
                data[i + 2] = x as u8;
                data[i + 3] = 255;
            }
        }
        data
    }

    #[test]
    fn the_region_is_cut_from_the_requested_corner() {
        let data = frame(100, 50, 400);
        let (w, h, gray) = crop_gray(&data, 400, 100, 50, [0.5, 0.2, 1.0, 0.6]).unwrap();
        assert_eq!((w, h), (50, 20));
        assert_eq!(gray.len(), 50 * 20);
        assert_eq!(gray[0], 50);
        assert_eq!(gray[49], 99);
    }

    #[test]
    fn rows_padded_beyond_the_width_are_handled() {
        let data = frame(10, 4, 64);
        let (w, h, gray) = crop_gray(&data, 64, 10, 4, [0.0, 0.0, 1.0, 1.0]).unwrap();
        assert_eq!((w, h, gray.len()), (10, 4, 40));
        assert_eq!(gray[9], 9);
    }

    #[test]
    fn white_and_black_map_to_the_ends_of_the_scale() {
        let mut data = vec![0u8; 8];
        data[4..8].copy_from_slice(&[255, 255, 255, 255]);
        let (_, _, gray) = crop_gray(&data, 8, 2, 1, [0.0, 0.0, 1.0, 1.0]).unwrap();
        assert_eq!(gray, vec![0, 255]);
    }

    #[test]
    fn empty_or_inside_out_regions_are_refused() {
        let data = frame(100, 50, 400);
        assert!(crop_gray(&data, 400, 100, 50, [0.5, 0.5, 0.5, 0.9]).is_none());
        assert!(crop_gray(&data, 400, 100, 50, [0.8, 0.1, 0.2, 0.9]).is_none());
        assert!(crop_gray(&data[..100], 400, 100, 50, [0.0, 0.0, 1.0, 1.0]).is_none());
    }

    #[test]
    fn regions_outside_the_picture_are_clamped() {
        let data = frame(100, 50, 400);
        let (w, h, _) = crop_gray(&data, 400, 100, 50, [-1.0, -1.0, 2.0, 2.0]).unwrap();
        assert_eq!((w, h), (100, 50));
    }
}
