//! Premiere time units and timecode math.

/// Premiere stores all times as ticks: 254,016,000,000 per second.
pub const TICKS_PER_SECOND: i64 = 254_016_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    pub num: i64,
    pub den: i64,
}

impl Rate {
    pub const fn new(num: i64, den: i64) -> Self {
        Self { num, den }
    }

    /// Parses ffprobe style rates such as `24000/1001` or `25`.
    pub fn parse(s: &str) -> Option<Self> {
        let (num, den) = match s.split_once('/') {
            Some((n, d)) => (n.trim().parse().ok()?, d.trim().parse().ok()?),
            None => (s.trim().parse().ok()?, 1),
        };
        (num > 0 && den > 0).then_some(Self { num, den })
    }

    pub fn frame_ticks(&self) -> i64 {
        TICKS_PER_SECOND * self.den / self.num
    }

    /// Whole frames per second used for timecode counting (23.976 counts as 24).
    pub fn nominal_fps(&self) -> i64 {
        (self.num + self.den / 2) / self.den
    }

    pub fn fps(&self) -> f64 {
        self.num as f64 / self.den as f64
    }

    /// Premiere's time display code for this rate (used for `TimeDisplay` / `TimecodeFormat`).
    pub fn premiere_time_display(&self, drop_frame: bool) -> u32 {
        let ntsc = self.den == 1001;
        match (self.nominal_fps(), ntsc, drop_frame) {
            (24, true, _) => 110,
            (24, false, _) => 100,
            (25, _, _) => 101,
            (30, true, true) => 102,
            (30, true, false) => 103,
            (30, false, _) => 104,
            (50, _, _) => 105,
            (60, true, true) => 106,
            (60, true, false) => 107,
            (60, false, _) => 108,
            _ => 110,
        }
    }
}

pub fn sample_ticks(sample_rate: u32) -> i64 {
    TICKS_PER_SECOND / sample_rate.max(1) as i64
}

pub fn floor_to(value: i64, step: i64) -> i64 {
    value.div_euclid(step) * step
}

/// Parses `HH:MM:SS:FF` (or `;` for drop-frame) into a frame count from midnight.
pub fn timecode_to_frames(tc: &str, rate: Rate) -> Option<i64> {
    let drop = is_drop_frame(tc);
    let parts: Vec<i64> = tc
        .split(|c| matches!(c, ':' | ';' | '.' | ','))
        .map(|p| p.trim().parse().ok())
        .collect::<Option<_>>()?;
    let [h, m, s, f] = parts[..] else { return None };
    let fps = rate.nominal_fps();
    let mut frames = ((h * 60 + m) * 60 + s) * fps + f;
    if drop && (fps == 30 || fps == 60) {
        let drop_per_min = fps / 15;
        let total_minutes = h * 60 + m;
        frames -= drop_per_min * (total_minutes - total_minutes / 10);
    }
    Some(frames)
}

pub fn is_drop_frame(tc: &str) -> bool {
    tc.contains(';') || tc.contains(',')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_ticks_match_premiere() {
        assert_eq!(Rate::new(24000, 1001).frame_ticks(), 10_594_584_000);
        assert_eq!(sample_ticks(48_000), 5_292_000);
    }

    #[test]
    fn timecode_matches_premiere_media_start() {
        let rate = Rate::new(24000, 1001);
        let frames = timecode_to_frames("02:57:11:17", rate).unwrap();
        assert_eq!(frames * rate.frame_ticks(), 2_703_324_648_024_000);
    }

    #[test]
    fn drop_frame_timecode() {
        let rate = Rate::new(30000, 1001);
        assert_eq!(timecode_to_frames("00:01:00;02", rate), Some(1800));
        assert_eq!(timecode_to_frames("00:10:00;00", rate), Some(17982));
    }
}
