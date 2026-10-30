pub const SIZES_MB: [u32; 6] = [8, 10, 25, 50, 100, 500];
pub const HEIGHTS: [u32; 4] = [1080, 720, 480, 360];
pub const FRAME_RATES: [u32; 3] = [60, 30, 24];
pub const MIN_MB: u32 = 1;
pub const MAX_MB: u32 = 500;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompressOptions {
    pub target_mb: u32,
    pub max_height: Option<u32>,
    pub max_fps: Option<u32>,
    pub mute: bool,
}

impl Default for CompressOptions {
    fn default() -> Self {
        Self {
            target_mb: 10,
            max_height: None,
            max_fps: None,
            mute: false,
        }
    }
}

impl CompressOptions {
    pub fn with_size(target_mb: u32) -> Self {
        Self {
            target_mb,
            ..Self::default()
        }
    }

    pub fn target_bytes(&self) -> u64 {
        self.target_mb as u64 * 1_000_000
    }

    pub fn changes_picture_or_sound(&self) -> bool {
        self.max_height.is_some() || self.max_fps.is_some() || self.mute
    }

    pub fn validate(&self) -> Result<(), String> {
        if !(MIN_MB..=MAX_MB).contains(&self.target_mb) {
            return Err(format!("choose a size between {MIN_MB} and {MAX_MB} MB"));
        }
        if self.max_height.is_some_and(|h| !(144..=4320).contains(&h)) {
            return Err("the picture height must be between 144 and 4320".into());
        }
        if self.max_fps.is_some_and(|f| !(5..=240).contains(&f)) {
            return Err("the frame rate must be between 5 and 240".into());
        }
        Ok(())
    }

    pub fn describe(&self) -> String {
        let mut parts = vec![format!("{} MB", self.target_mb)];
        if let Some(h) = self.max_height {
            parts.push(format!("up to {h}p"));
        }
        if let Some(f) = self.max_fps {
            parts.push(format!("up to {f} fps"));
        }
        if self.mute {
            parts.push("no sound".into());
        }
        parts.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_a_plain_ten_megabyte_squeeze() {
        let o = CompressOptions::default();
        assert_eq!(o.target_mb, 10);
        assert_eq!(o.target_bytes(), 10_000_000);
        assert!(!o.changes_picture_or_sound());
        assert!(o.validate().is_ok());
    }

    #[test]
    fn sizes_outside_the_range_are_refused() {
        assert!(CompressOptions::with_size(0).validate().is_err());
        assert!(CompressOptions::with_size(501).validate().is_err());
        assert!(CompressOptions::with_size(1).validate().is_ok());
        assert!(CompressOptions::with_size(500).validate().is_ok());
    }

    #[test]
    fn silly_caps_are_refused() {
        let mut o = CompressOptions {
            max_height: Some(50),
            ..Default::default()
        };
        assert!(o.validate().is_err());
        o.max_height = Some(720);
        o.max_fps = Some(1000);
        assert!(o.validate().is_err());
        o.max_fps = Some(30);
        assert!(o.validate().is_ok());
    }

    #[test]
    fn every_preset_is_valid() {
        for mb in SIZES_MB {
            assert!(CompressOptions::with_size(mb).validate().is_ok());
        }
        for h in HEIGHTS {
            let o = CompressOptions {
                max_height: Some(h),
                ..Default::default()
            };
            assert!(o.validate().is_ok());
        }
        for f in FRAME_RATES {
            let o = CompressOptions {
                max_fps: Some(f),
                ..Default::default()
            };
            assert!(o.validate().is_ok());
        }
    }

    #[test]
    fn the_description_lists_only_what_was_chosen() {
        assert_eq!(CompressOptions::default().describe(), "10 MB");
        let o = CompressOptions {
            target_mb: 50,
            max_height: Some(720),
            max_fps: Some(30),
            mute: true,
        };
        assert_eq!(o.describe(), "50 MB, up to 720p, up to 30 fps, no sound");
    }
}
