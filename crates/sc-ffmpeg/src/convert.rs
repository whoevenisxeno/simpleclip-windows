use crate::{Error, Result};
use ffmpeg_next::ffi;
use std::ptr;

// libswscale/swscale.h `#define SWS_BILINEAR 2`; bindgen skips flag defines, and
// this is a stable public ABI value.
const SWS_BILINEAR: i32 = 2;

/// Converts captured BGRA frames (what the Wayland portal delivers) into tightly
/// packed NV12 (what the VA-API encoder uploads). Wraps one reusable swscale
/// context; the output has stride == width so it feeds `Recorder::push_nv12`.
pub struct Bgra2Nv12 {
    sws: *mut ffi::SwsContext,
    width: i32,
    height: i32,
}

// Owned by, and used from, a single encode thread; moving it there is sound.
unsafe impl Send for Bgra2Nv12 {}

impl Bgra2Nv12 {
    pub fn new(width: u32, height: u32) -> Result<Self> {
        let (w, h) = (width as i32, height as i32);
        // # Safety: sws_getContext validates its own args and returns null on
        // failure, which we turn into an error rather than deref.
        let sws = unsafe {
            ffi::sws_getContext(
                w,
                h,
                ffi::AVPixelFormat::AV_PIX_FMT_BGRA,
                w,
                h,
                ffi::AVPixelFormat::AV_PIX_FMT_NV12,
                SWS_BILINEAR,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        if sws.is_null() {
            return Err(Error::Av {
                code: -1,
                ctx: "sws_getContext(bgra->nv12)",
            });
        }
        Ok(Self {
            sws,
            width: w,
            height: h,
        })
    }
}

impl Bgra2Nv12 {
    /// Convert one BGRA frame (with its source row stride in bytes) into a fresh
    /// packed NV12 buffer. Row stride comes from the capture backend; the Wayland
    /// portal pads rows, so we can't assume width*4.
    pub fn convert(&mut self, bgra: &[u8], src_stride: usize) -> Result<Vec<u8>> {
        let (w, h) = (self.width as usize, self.height as usize);
        if bgra.len() < src_stride * h {
            return Err(Error::Ffmpeg(format!(
                "short BGRA frame: {} < {}",
                bgra.len(),
                src_stride * h
            )));
        }
        let mut out = vec![0u8; w * h + w * h / 2];
        let (y_plane, uv_plane) = out.split_at_mut(w * h);
        let src_data = [bgra.as_ptr(), ptr::null(), ptr::null(), ptr::null()];
        let src_stride = [src_stride as i32, 0, 0, 0];
        let dst_data = [
            y_plane.as_mut_ptr(),
            uv_plane.as_mut_ptr(),
            ptr::null_mut(),
            ptr::null_mut(),
        ];
        let dst_stride = [self.width, self.width, 0, 0];
        // # Safety: all four plane arrays are the fixed length libswscale reads;
        // src/dst buffers are sized above; sws context matches these dimensions.
        let rc = unsafe {
            ffi::sws_scale(
                self.sws,
                src_data.as_ptr(),
                src_stride.as_ptr(),
                0,
                self.height,
                dst_data.as_ptr(),
                dst_stride.as_ptr(),
            )
        };
        if rc != self.height {
            return Err(Error::Av {
                code: rc,
                ctx: "sws_scale",
            });
        }
        Ok(out)
    }
}

impl Drop for Bgra2Nv12 {
    fn drop(&mut self) {
        unsafe { ffi::sws_freeContext(self.sws) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // swscale is available in CI (libswscale-dev installed); this needs no GPU.
    #[test]
    fn converts_solid_bgra_to_nv12_luma() {
        let (w, h) = (64u32, 48u32);
        let stride = (w * 4) as usize;
        // Solid white BGRA (B=G=R=255, A=255) -> Y should be near 235 (limited range).
        let bgra = vec![255u8; stride * h as usize];
        let mut conv = Bgra2Nv12::new(w, h).expect("ctx");
        let nv12 = conv.convert(&bgra, stride).expect("convert");
        assert_eq!(nv12.len(), (w * h + w * h / 2) as usize);
        let y0 = nv12[0];
        assert!(y0 > 200, "white luma should be high, got {y0}");
    }

    #[test]
    fn rejects_short_frame() {
        let mut conv = Bgra2Nv12::new(64, 48).unwrap();
        assert!(conv.convert(&[0u8; 10], 256).is_err());
    }
}
