use crate::{Error, Result};
use ffmpeg_next::ffi;
use std::ptr;

/// Create a VA-API hardware device context. Caller owns the returned ref and
/// must `av_buffer_unref` it. Returns an error (not UB) if no VA-API device can
/// be opened, which is how we detect "no usable hardware encoder" up front.
///
/// # Safety
/// Wraps `av_hwdevice_ctx_create`; the out-pointer is null-initialized and only
/// read on success, matching the FFmpeg contract.
pub fn create_vaapi_device() -> Result<*mut ffi::AVBufferRef> {
    unsafe {
        let mut dev: *mut ffi::AVBufferRef = ptr::null_mut();
        let rc = ffi::av_hwdevice_ctx_create(
            &mut dev,
            ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
            ptr::null(),
            ptr::null_mut(),
            0,
        );
        if rc < 0 || dev.is_null() {
            return Err(Error::Av {
                code: rc,
                ctx: "av_hwdevice_ctx_create(vaapi)",
            });
        }
        Ok(dev)
    }
}

/// Build a VA-API hardware frame pool sized for `width`x`height`, with NV12 as
/// the software (upload) format. Caller owns the returned ref. The encoder holds
/// its own ref, so we unref our copy after wiring it up.
///
/// # Safety
/// `device` must be a live VA-API device ref from `create_vaapi_device`. We
/// initialize every field FFmpeg requires before `av_hwframe_ctx_init`.
pub fn create_hw_frames_ctx(
    device: *mut ffi::AVBufferRef,
    width: i32,
    height: i32,
) -> Result<*mut ffi::AVBufferRef> {
    unsafe {
        let frames = ffi::av_hwframe_ctx_alloc(device);
        if frames.is_null() {
            return Err(Error::Av {
                code: -1,
                ctx: "av_hwframe_ctx_alloc",
            });
        }
        let ctx = (*frames).data as *mut ffi::AVHWFramesContext;
        (*ctx).format = ffi::AVPixelFormat::AV_PIX_FMT_VAAPI;
        (*ctx).sw_format = ffi::AVPixelFormat::AV_PIX_FMT_NV12;
        (*ctx).width = width;
        (*ctx).height = height;
        (*ctx).initial_pool_size = 20;
        let rc = ffi::av_hwframe_ctx_init(frames);
        if rc < 0 {
            let mut f = frames;
            ffi::av_buffer_unref(&mut f);
            return Err(Error::Av {
                code: rc,
                ctx: "av_hwframe_ctx_init",
            });
        }
        Ok(frames)
    }
}
