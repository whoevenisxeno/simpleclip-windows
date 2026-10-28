use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};

static UPDATE_WAV: &[u8] = include_bytes!("../../../assets/sounds/update.wav");

pub fn play_update() {
    unsafe {
        let _ = PlaySoundW(
            windows::core::PCWSTR(UPDATE_WAV.as_ptr() as *const u16),
            None,
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        );
    }
}
