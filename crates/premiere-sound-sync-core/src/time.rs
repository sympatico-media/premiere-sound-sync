//! Premiere time units and timecode math.

use std::time::SystemTime;

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

/// `t`'s calendar date in UTC, as `YYYY-MM-DD`. Used to bucket files into shoot days; a file's
/// filesystem modification time is a reasonable stand-in for its recording date on camera
/// originals that haven't been copied through something that rewrites timestamps.
pub fn ymd_utc(t: SystemTime) -> Option<String> {
    let days = t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64 / 86_400;
    let (y, m, d) = civil_from_days(days);
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

/// Builds a UTC [`SystemTime`] from a calendar date and time of day, e.g. parsed out of
/// ffprobe's `creation_time` tag or a BWF file's bext `OriginationDate`/`OriginationTime`.
/// Treated as UTC regardless of where the recorder's clock was actually set; for day-bucketing
/// shoot footage that's close enough, and it keeps this dependency-free.
pub fn system_time_from_ymd_hms(y: i64, m: u32, d: u32, hh: u32, mm: u32, ss: u32) -> Option<SystemTime> {
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    let days = days_from_civil(y, m, d);
    let secs = days * 86_400 + hh as i64 * 3600 + mm as i64 * 60 + ss as i64;
    if secs >= 0 {
        Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs as u64))
    } else {
        std::time::UNIX_EPOCH.checked_sub(std::time::Duration::from_secs((-secs) as u64))
    }
}

/// (year, month, day) to days-since-epoch, the inverse of [`civil_from_days`] (same source).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64; // [0, 399]
    let mp = if m > 2 { m - 3 } else { m + 9 }; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d as u32 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy as u64; // [0, 146096]
    era * 146_097 + doe as i64 - 719_468
}

/// Days-since-epoch to (year, month, day), Howard Hinnant's public-domain algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
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

    #[test]
    fn ymd_matches_known_dates() {
        assert_eq!(ymd_utc(std::time::UNIX_EPOCH), Some("1970-01-01".to_string()));
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_757_462_400); // 2025-09-10 UTC
        assert_eq!(ymd_utc(t), Some("2025-09-10".to_string()));
    }

    #[test]
    fn system_time_from_ymd_hms_round_trips_through_ymd_utc() {
        let t = system_time_from_ymd_hms(2025, 9, 10, 14, 23, 1).unwrap();
        assert_eq!(ymd_utc(t), Some("2025-09-10".to_string()));
        assert_eq!(system_time_from_ymd_hms(1970, 1, 1, 0, 0, 0), Some(std::time::UNIX_EPOCH));
        assert_eq!(system_time_from_ymd_hms(2025, 13, 1, 0, 0, 0), None);
    }
}
