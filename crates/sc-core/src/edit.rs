pub const MIN_SEGMENT: f64 = 0.1;
pub const MIN_CROP: f64 = 0.08;
pub const SPEED_MIN: f64 = 0.25;
pub const SPEED_MAX: f64 = 4.0;
pub const VOLUME_MAX: f32 = 2.0;
pub const FADE_MAX: f64 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
}

impl Segment {
    pub fn len(&self) -> f64 {
        self.end - self.start
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Crop {
    pub fn full() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        }
    }

    pub fn is_full(&self) -> bool {
        self.x <= 0.0005 && self.y <= 0.0005 && self.w >= 0.999 && self.h >= 0.999
    }

    pub fn clamped(self) -> Self {
        let w = self.w.clamp(MIN_CROP, 1.0);
        let h = self.h.clamp(MIN_CROP, 1.0);
        Self {
            x: self.x.clamp(0.0, 1.0 - w),
            y: self.y.clamp(0.0, 1.0 - h),
            w,
            h,
        }
    }

    pub fn with_ratio(frame_w: f64, frame_h: f64, ratio: f64) -> Self {
        let frame = frame_w / frame_h.max(1.0);
        let (w, h) = if ratio >= frame {
            (1.0, frame / ratio)
        } else {
            (ratio / frame, 1.0)
        };
        Self {
            x: (1.0 - w) / 2.0,
            y: (1.0 - h) / 2.0,
            w,
            h,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub segments: Vec<Segment>,
    pub speed: f64,
    pub volume: f32,
    pub fade_in: f64,
    pub fade_out: f64,
    pub crop: Option<Crop>,
    pub turn: u16,
    pub flip_h: bool,
    pub flip_v: bool,
}

impl Default for Plan {
    fn default() -> Self {
        Self::whole(0.0)
    }
}

impl Plan {
    pub fn whole(duration: f64) -> Self {
        Self {
            segments: vec![Segment {
                start: 0.0,
                end: duration.max(MIN_SEGMENT),
            }],
            speed: 1.0,
            volume: 1.0,
            fade_in: 0.0,
            fade_out: 0.0,
            crop: None,
            turn: 0,
            flip_h: false,
            flip_v: false,
        }
    }

    pub fn kept_secs(&self) -> f64 {
        self.segments.iter().map(Segment::len).sum()
    }

    pub fn output_secs(&self) -> f64 {
        self.kept_secs() / self.speed.clamp(SPEED_MIN, SPEED_MAX)
    }

    pub fn bounds(&self) -> (f64, f64) {
        match (self.segments.first(), self.segments.last()) {
            (Some(a), Some(b)) => (a.start, b.end),
            _ => (0.0, 0.0),
        }
    }

    pub fn segment_at(&self, t: f64) -> Option<usize> {
        self.segments.iter().position(|s| t >= s.start && t < s.end)
    }

    pub fn next_start(&self, t: f64) -> Option<f64> {
        self.segments
            .iter()
            .map(|s| s.start)
            .find(|start| *start > t)
    }

    pub fn split(&mut self, at: f64) -> Option<usize> {
        let i = self.segment_at(at)?;
        let s = self.segments[i];
        if at - s.start < MIN_SEGMENT || s.end - at < MIN_SEGMENT {
            return None;
        }
        self.segments[i].end = at;
        self.segments.insert(
            i + 1,
            Segment {
                start: at,
                end: s.end,
            },
        );
        Some(i + 1)
    }

    pub fn remove(&mut self, index: usize) -> bool {
        if self.segments.len() < 2 || index >= self.segments.len() {
            return false;
        }
        self.segments.remove(index);
        true
    }

    pub fn move_edge(&mut self, index: usize, start_edge: bool, value: f64, duration: f64) {
        let Some(seg) = self.segments.get(index).copied() else {
            return;
        };
        let floor = if index == 0 {
            0.0
        } else {
            self.segments[index - 1].end
        };
        let ceil = self
            .segments
            .get(index + 1)
            .map(|s| s.start)
            .unwrap_or(duration);
        if start_edge {
            self.segments[index].start = value.clamp(floor, seg.end - MIN_SEGMENT);
        } else {
            self.segments[index].end = value.clamp(seg.start + MIN_SEGMENT, ceil);
        }
    }

    pub fn clamp_to(&mut self, duration: f64) {
        for s in &mut self.segments {
            s.start = s.start.clamp(0.0, duration);
            s.end = s.end.clamp(0.0, duration);
        }
        self.segments.retain(|s| s.len() >= MIN_SEGMENT * 0.5);
        self.segments.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut merged: Vec<Segment> = Vec::new();
        for s in self.segments.drain(..) {
            match merged.last_mut() {
                Some(last) if s.start <= last.end + 1e-6 => last.end = last.end.max(s.end),
                _ => merged.push(s),
            }
        }
        if merged.is_empty() {
            merged.push(Segment {
                start: 0.0,
                end: duration.max(MIN_SEGMENT),
            });
        }
        self.segments = merged;
        self.speed = self.speed.clamp(SPEED_MIN, SPEED_MAX);
        self.volume = self.volume.clamp(0.0, VOLUME_MAX);
        self.fade_in = self.fade_in.clamp(0.0, FADE_MAX);
        self.fade_out = self.fade_out.clamp(0.0, FADE_MAX);
        self.turn %= 360;
        self.crop = self.crop.map(Crop::clamped).filter(|c| !c.is_full());
    }

    pub fn output_time(&self, t: f64) -> Option<f64> {
        let i = self.segment_at(t)?;
        let before: f64 = self.segments[..i].iter().map(Segment::len).sum();
        Some((before + t - self.segments[i].start) / self.speed.clamp(SPEED_MIN, SPEED_MAX))
    }

    pub fn is_plain_cut(&self) -> bool {
        self.segments.len() == 1
            && (self.speed - 1.0).abs() < 1e-6
            && (self.volume - 1.0).abs() < 1e-6
            && self.fade_in <= 0.0
            && self.fade_out <= 0.0
            && !self.changes_picture()
    }

    pub fn changes_picture(&self) -> bool {
        self.crop.is_some() || self.turn != 0 || self.flip_h || self.flip_v
    }

    pub fn needs_pixels(&self) -> bool {
        self.changes_picture() || self.fade_in > 0.0 || self.fade_out > 0.0
    }

    pub fn oriented(&self, w: u32, h: u32) -> (u32, u32) {
        if self.turn % 180 == 90 {
            (h, w)
        } else {
            (w, h)
        }
    }

    pub fn crop_pixels(&self, w: u32, h: u32) -> (u32, u32, u32, u32) {
        let (ow, oh) = self.oriented(w, h);
        let Some(c) = self.crop else {
            return (0, 0, ow & !1, oh & !1);
        };
        let c = c.clamped();
        let even = |v: f64| ((v.round() as u32) & !1).max(2);
        let cw = even(c.w * ow as f64).min(ow & !1);
        let ch = even(c.h * oh as f64).min(oh & !1);
        let x = (even(c.x * ow as f64)).min(ow.saturating_sub(cw));
        let y = (even(c.y * oh as f64)).min(oh.saturating_sub(ch));
        (x, y, cw, ch)
    }

    pub fn output_size(&self, w: u32, h: u32) -> (u32, u32) {
        let (_, _, cw, ch) = self.crop_pixels(w, h);
        (cw, ch)
    }

    pub fn fade_gain(&self, out_t: f64, out_total: f64) -> f32 {
        let mut g = 1.0f64;
        if self.fade_in > 0.0 {
            g = g.min(out_t / self.fade_in);
        }
        if self.fade_out > 0.0 {
            g = g.min((out_total - out_t) / self.fade_out);
        }
        g.clamp(0.0, 1.0) as f32
    }
}

pub fn orient(
    src: &[u8],
    w: usize,
    h: usize,
    turn: u16,
    flip_h: bool,
    flip_v: bool,
) -> (Vec<u8>, usize, usize) {
    let (dw, dh) = if turn % 180 == 90 { (h, w) } else { (w, h) };
    let mut out = vec![0u8; dw * dh * 4];
    for y in 0..dh {
        for x in 0..dw {
            let (mut sx, mut sy) = match turn % 360 {
                90 => (y, h - 1 - x),
                180 => (w - 1 - x, h - 1 - y),
                270 => (w - 1 - y, x),
                _ => (x, y),
            };
            if flip_h {
                sx = w - 1 - sx;
            }
            if flip_v {
                sy = h - 1 - sy;
            }
            let s = (sy * w + sx) * 4;
            let d = (y * dw + x) * 4;
            out[d..d + 4].copy_from_slice(&src[s..s + 4]);
        }
    }
    (out, dw, dh)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> Plan {
        Plan::whole(10.0)
    }

    #[test]
    fn a_whole_clip_is_a_plain_cut() {
        let p = plan();
        assert!(p.is_plain_cut());
        assert_eq!(p.output_secs(), 10.0);
    }

    #[test]
    fn splitting_makes_two_pieces_that_still_add_up() {
        let mut p = plan();
        assert_eq!(p.split(4.0), Some(1));
        assert_eq!(p.segments.len(), 2);
        assert_eq!(p.kept_secs(), 10.0);
        assert!(!p.is_plain_cut());
    }

    #[test]
    fn a_split_too_close_to_an_edge_is_refused() {
        let mut p = plan();
        assert_eq!(p.split(0.05), None);
        assert_eq!(p.split(9.98), None);
        assert_eq!(p.segments.len(), 1);
    }

    #[test]
    fn removing_a_piece_leaves_a_gap_and_shortens_the_output() {
        let mut p = plan();
        p.split(3.0);
        p.split(6.0);
        assert!(p.remove(1));
        assert_eq!(p.kept_secs(), 7.0);
        assert_eq!(p.segment_at(4.0), None);
        assert_eq!(p.next_start(4.0), Some(6.0));
    }

    #[test]
    fn the_last_piece_cannot_be_removed() {
        let mut p = plan();
        assert!(!p.remove(0));
    }

    #[test]
    fn output_time_skips_the_cuts_and_follows_the_speed() {
        let mut p = plan();
        p.split(3.0);
        p.split(6.0);
        p.remove(1);
        assert_eq!(p.output_time(1.0), Some(1.0));
        assert_eq!(p.output_time(4.0), None);
        assert_eq!(p.output_time(7.0), Some(4.0));
        p.speed = 2.0;
        assert_eq!(p.output_time(7.0), Some(2.0));
        assert_eq!(p.output_secs(), 3.5);
    }

    #[test]
    fn dragging_an_edge_stops_at_the_neighbour() {
        let mut p = plan();
        p.split(5.0);
        p.move_edge(0, false, 9.0, 10.0);
        assert_eq!(p.segments[0].end, 5.0);
        p.move_edge(1, true, 1.0, 10.0);
        assert_eq!(p.segments[1].start, 5.0);
        p.move_edge(1, false, 9.0, 10.0);
        assert_eq!(p.segments[1].end, 9.0);
        p.move_edge(1, true, 8.95, 10.0);
        assert!((p.segments[1].start - 8.9).abs() < 1e-9);
    }

    #[test]
    fn clamping_merges_overlaps_and_keeps_something() {
        let mut p = plan();
        p.segments = vec![
            Segment {
                start: 5.0,
                end: 8.0,
            },
            Segment {
                start: 1.0,
                end: 6.0,
            },
            Segment {
                start: 12.0,
                end: 14.0,
            },
        ];
        p.clamp_to(10.0);
        assert_eq!(
            p.segments,
            vec![Segment {
                start: 1.0,
                end: 8.0
            }]
        );
        p.segments.clear();
        p.clamp_to(10.0);
        assert_eq!(p.segments.len(), 1);
    }

    #[test]
    fn ratio_crops_are_centred_and_fit_the_frame() {
        let c = Crop::with_ratio(1920.0, 1080.0, 1.0);
        assert!((c.w - 0.5625).abs() < 1e-9 && c.h == 1.0);
        assert!((c.x - 0.21875).abs() < 1e-9);
        let c = Crop::with_ratio(1920.0, 1080.0, 9.0 / 16.0);
        assert!((c.w - 0.31640625).abs() < 1e-9);
        let c = Crop::with_ratio(1000.0, 1000.0, 2.0);
        assert!((c.h - 0.5).abs() < 1e-9 && c.w == 1.0);
    }

    #[test]
    fn crop_pixels_are_even_and_inside_the_picture() {
        let mut p = plan();
        p.crop = Some(Crop {
            x: 0.1,
            y: 0.1,
            w: 0.5,
            h: 0.5,
        });
        let (x, y, w, h) = p.crop_pixels(1920, 1080);
        assert!(x % 2 == 0 && y % 2 == 0 && w % 2 == 0 && h % 2 == 0);
        assert!(x + w <= 1920 && y + h <= 1080);
        assert_eq!((w, h), (960, 540));
        p.turn = 90;
        assert_eq!(p.oriented(1920, 1080), (1080, 1920));
        assert_eq!(p.output_size(1920, 1080), (540, 960));
    }

    #[test]
    fn a_full_frame_crop_is_dropped() {
        let mut p = plan();
        p.crop = Some(Crop::full());
        p.clamp_to(10.0);
        assert!(p.crop.is_none());
    }

    #[test]
    fn fades_ramp_in_and_out() {
        let mut p = plan();
        p.fade_in = 2.0;
        p.fade_out = 2.0;
        assert_eq!(p.fade_gain(0.0, 10.0), 0.0);
        assert_eq!(p.fade_gain(1.0, 10.0), 0.5);
        assert_eq!(p.fade_gain(5.0, 10.0), 1.0);
        assert_eq!(p.fade_gain(9.0, 10.0), 0.5);
        assert!(p.needs_pixels());
    }

    fn pixels(values: &[u8]) -> Vec<u8> {
        values.iter().flat_map(|v| [*v, 0, 0, 255]).collect()
    }

    fn reds(buf: &[u8]) -> Vec<u8> {
        buf.as_chunks::<4>().0.iter().map(|p| p[0]).collect()
    }

    #[test]
    fn turning_clockwise_moves_the_top_left_to_the_top_right() {
        let src = pixels(&[1, 2, 3, 4, 5, 6]);
        let (out, w, h) = orient(&src, 3, 2, 90, false, false);
        assert_eq!((w, h), (2, 3));
        assert_eq!(reds(&out), vec![4, 1, 5, 2, 6, 3]);
        let (out, w, h) = orient(&src, 3, 2, 180, false, false);
        assert_eq!((w, h), (3, 2));
        assert_eq!(reds(&out), vec![6, 5, 4, 3, 2, 1]);
        let (out, _, _) = orient(&src, 3, 2, 270, false, false);
        assert_eq!(reds(&out), vec![3, 6, 2, 5, 1, 4]);
    }

    #[test]
    fn flips_happen_before_the_turn() {
        let src = pixels(&[1, 2, 3, 4, 5, 6]);
        let (out, _, _) = orient(&src, 3, 2, 0, true, false);
        assert_eq!(reds(&out), vec![3, 2, 1, 6, 5, 4]);
        let (out, _, _) = orient(&src, 3, 2, 0, false, true);
        assert_eq!(reds(&out), vec![4, 5, 6, 1, 2, 3]);
        let (flipped, w, h) = orient(&src, 3, 2, 0, true, false);
        let (twice, _, _) = orient(&flipped, w, h, 90, false, false);
        let (once, _, _) = orient(&src, 3, 2, 90, true, false);
        assert_eq!(reds(&once), reds(&twice));
    }
}
