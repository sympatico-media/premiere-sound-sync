//! Pairing of video with production audio, and multicam grouping.
//!
//! Two independent methods can find a pair, and both can run together: timecode (matching
//! embedded timecode overlap) and waveform (matching the sound itself, for clips with no usable
//! timecode). Either can be switched off; at least one is expected to be on.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::Duration;

use crate::media::{MediaFile, MediaKind};
use crate::time::TICKS_PER_SECOND;
use crate::waveform::{self, Fingerprint};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SyncMethod {
    /// Paired by embedded timecode overlap.
    Timecode,
    /// Paired by matching the sound itself. `confidence` is the ratio of the best correlation
    /// peak to the next-best, unrelated peak; higher means a safer match.
    Waveform { confidence: f64 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct MergedPair {
    pub video: usize,
    pub audio: usize,
    pub method: SyncMethod,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MulticamGroup {
    /// Angles: one entry per camera, each holding that camera's clips in time order.
    pub angles: Vec<Vec<usize>>,
    pub audio: Option<usize>,
}

/// Which sync method(s) to use, and whether to additionally group multicam angles.
#[derive(Debug, Clone, Copy)]
pub struct SyncOptions {
    /// Pair video to audio by embedded timecode overlap.
    pub timecode: bool,
    /// Pair video to audio by matching the sound itself. Requires `fingerprints` to be populated
    /// for the files involved; anything missing a fingerprint is simply skipped.
    pub waveform: bool,
    pub multicam: bool,
}

impl Default for SyncOptions {
    fn default() -> Self {
        Self {
            timecode: true,
            waveform: false,
            multicam: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SyncPlan {
    pub merged: Vec<MergedPair>,
    pub multicam: Vec<MulticamGroup>,
    /// Files that didn't sync with anything; imported as plain clips.
    pub unsynced: Vec<usize>,
    /// Each file's position in ticks on the shared sync timeline, however it was found. `None`
    /// when no sync method placed the file anywhere.
    pub positions: Vec<Option<i64>>,
}

impl SyncPlan {
    pub fn audio_for(&self, video: usize) -> Option<usize> {
        self.merged.iter().find(|m| m.video == video).map(|m| m.audio)
    }
}

fn overlap(a: &MediaFile, b: &MediaFile) -> i64 {
    match (a.start, a.end(), b.start, b.end()) {
        (Some(s1), Some(e1), Some(s2), Some(e2)) => e1.min(e2) - s1.max(s2),
        _ => 0,
    }
}

/// Requires at least one second of overlap, or half of the shorter clip for very short clips.
fn is_meaningful(a: &MediaFile, b: &MediaFile, ov: i64) -> bool {
    let shorter = a.duration.min(b.duration);
    ov > 0 && ov >= TICKS_PER_SECOND.min(shorter / 2)
}

/// Same overlap test as [`overlap`], but against the shared sync timeline instead of raw
/// timecode, so it also works for pairs placed there by waveform sync.
fn positions_overlap(files: &[MediaFile], positions: &[Option<i64>], a: usize, b: usize) -> i64 {
    match (positions[a], positions[b]) {
        (Some(pa), Some(pb)) => (pa + files[a].duration).min(pb + files[b].duration) - pa.max(pb),
        _ => 0,
    }
}

/// Narrows the audio files worth correlating a video against. Falls back to every audio file
/// when nothing narrows the search, so sparse metadata never hides a real match.
fn candidates_for(v: &MediaFile, audios: &[usize], files: &[MediaFile]) -> Vec<usize> {
    let has_slate = v.slate.scene.is_some() && v.slate.take.is_some();
    let slate_matches = |a: &MediaFile| has_slate && a.slate.scene == v.slate.scene && a.slate.take == v.slate.take;
    let time_close = |a: &MediaFile| match (v.modified, a.modified) {
        (Some(tv), Some(ta)) => {
            let diff = if tv >= ta { tv.duration_since(ta) } else { ta.duration_since(tv) };
            diff.map(|d| d <= Duration::from_secs(6 * 3600)).unwrap_or(false)
        }
        _ => false,
    };
    let narrowed: Vec<usize> = audios.iter().copied().filter(|&a| slate_matches(&files[a]) || time_close(&files[a])).collect();
    if narrowed.is_empty() {
        audios.to_vec()
    } else {
        narrowed
    }
}

pub fn plan(files: &[MediaFile], fingerprints: &HashMap<PathBuf, Fingerprint>, opts: &SyncOptions) -> SyncPlan {
    let videos: Vec<usize> = (0..files.len())
        .filter(|&i| files[i].kind == MediaKind::Video)
        .collect();
    let audios: Vec<usize> = (0..files.len())
        .filter(|&i| files[i].kind == MediaKind::Audio)
        .collect();

    // Timecode seeds every file's position up front: this keeps camera-to-camera multicam
    // grouping (which has no audio to anchor to) working exactly as it did before waveform sync
    // existed, whether or not a given file ends up paired to audio.
    let mut positions: Vec<Option<i64>> = vec![None; files.len()];
    if opts.timecode {
        for (i, f) in files.iter().enumerate() {
            positions[i] = f.start;
        }
    }

    let mut merged: Vec<MergedPair> = Vec::new();
    if opts.timecode {
        for &v in &videos {
            let best = audios
                .iter()
                .map(|&a| (a, overlap(&files[v], &files[a])))
                .filter(|&(a, ov)| is_meaningful(&files[v], &files[a], ov))
                .max_by_key(|&(_, ov)| ov);
            if let Some((a, _)) = best {
                merged.push(MergedPair { video: v, audio: a, method: SyncMethod::Timecode });
            }
        }
    }

    if opts.waveform {
        let matched: BTreeSet<usize> = merged.iter().map(|m| m.video).collect();
        for &v in &videos {
            if matched.contains(&v) {
                continue;
            }
            let Some(vfp) = fingerprints.get(&files[v].path) else { continue };
            let mut best: Option<(usize, waveform::Match)> = None;
            for a in candidates_for(&files[v], &audios, files) {
                let Some(afp) = fingerprints.get(&files[a].path) else { continue };
                if let Some(m) = waveform::correlate(vfp, afp) {
                    if best.as_ref().map_or(true, |(_, bm)| m.confidence > bm.confidence) {
                        best = Some((a, m));
                    }
                }
            }
            if let Some((a, m)) = best {
                let audio_position = positions[a].or(files[a].start).unwrap_or(0);
                positions[a] = Some(audio_position);
                positions[v] = Some(audio_position + m.offset_ticks);
                merged.push(MergedPair { video: v, audio: a, method: SyncMethod::Waveform { confidence: m.confidence } });
            }
        }
    }

    let mut groups = Vec::new();
    if opts.multicam {
        let mut parent: Vec<usize> = (0..files.len()).collect();
        fn find(p: &mut [usize], x: usize) -> usize {
            let mut r = x;
            while p[r] != r {
                r = p[r];
            }
            let mut c = x;
            while p[c] != r {
                let n = p[c];
                p[c] = r;
                c = n;
            }
            r
        }
        let audio_of = |v: usize| merged.iter().find(|m| m.video == v).map(|m| m.audio);
        for (i, &a) in videos.iter().enumerate() {
            for &b in &videos[i + 1..] {
                let linked = match (audio_of(a), audio_of(b)) {
                    (Some(x), Some(y)) => x == y,
                    (None, None) => {
                        let (ca, cb) = (&files[a].camera, &files[b].camera);
                        ca.is_some() && cb.is_some() && ca != cb && is_meaningful(&files[a], &files[b], positions_overlap(files, &positions, a, b))
                    }
                    _ => false,
                };
                if linked {
                    let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                    parent[ra] = rb;
                }
            }
        }
        let mut clusters: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for &v in &videos {
            let root = find(&mut parent, v);
            clusters.entry(root).or_default().push(v);
        }
        for members in clusters.into_values() {
            let mut by_camera: BTreeMap<String, Vec<usize>> = BTreeMap::new();
            for &v in &members {
                let cam = files[v]
                    .camera
                    .clone()
                    .unwrap_or_else(|| files[v].path.to_string_lossy().into_owned());
                by_camera.entry(cam).or_default().push(v);
            }
            if by_camera.len() < 2 {
                continue;
            }
            let mut angles: Vec<Vec<usize>> = by_camera.into_values().collect();
            for angle in &mut angles {
                angle.sort_by_key(|&v| positions[v]);
            }
            let audio = members.iter().find_map(|&v| audio_of(v));
            groups.push(MulticamGroup { angles, audio });
        }
    }

    let used: BTreeSet<usize> = merged
        .iter()
        .flat_map(|m| [m.video, m.audio])
        .chain(groups.iter().flat_map(|g| g.angles.iter().flatten().copied()))
        .collect();
    let unsynced = (0..files.len()).filter(|i| !used.contains(i)).collect();
    SyncPlan {
        merged,
        multicam: groups,
        unsynced,
        positions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::Slate;
    use crate::time::sample_ticks;
    use crate::waveform::synthetic_signal;
    use std::path::PathBuf;

    fn file(name: &str, kind: MediaKind, start_s: i64, dur_s: i64, camera: Option<&str>) -> MediaFile {
        MediaFile {
            path: PathBuf::from(name),
            kind,
            start: Some(start_s * TICKS_PER_SECOND),
            duration: dur_s * TICKS_PER_SECOND,
            rate: None,
            drop_frame: false,
            width: 0,
            height: 0,
            audio_channels: 2,
            sample_rate: 48_000,
            channel_names: vec![],
            camera: camera.map(str::to_string),
            reel: None,
            slate: Slate::default(),
            modified: None,
            recorded_at_source: None,
        }
    }

    fn opts(timecode: bool, waveform: bool, multicam: bool) -> SyncOptions {
        SyncOptions { timecode, waveform, multicam }
    }

    #[test]
    fn pairs_by_largest_overlap() {
        let files = vec![
            file("A001.mov", MediaKind::Video, 100, 60, Some("A")),
            file("1-001.wav", MediaKind::Audio, 95, 30, None),
            file("1-002.wav", MediaKind::Audio, 130, 60, None),
            file("lonely.wav", MediaKind::Audio, 9000, 10, None),
        ];
        let plan_result = plan(&files, &HashMap::new(), &opts(true, false, true));
        assert_eq!(plan_result.merged, vec![MergedPair { video: 0, audio: 2, method: SyncMethod::Timecode }]);
        assert!(plan_result.multicam.is_empty());
        assert_eq!(plan_result.unsynced, vec![1, 3]);
        assert_eq!(plan_result.positions[0], files[0].start);
        assert_eq!(plan_result.positions[2], files[2].start);
    }

    #[test]
    fn groups_cameras_sharing_a_take() {
        let files = vec![
            file("A001.mov", MediaKind::Video, 100, 60, Some("A")),
            file("B001.mov", MediaKind::Video, 102, 55, Some("B")),
            file("2-001.wav", MediaKind::Audio, 98, 70, None),
            file("A002.mov", MediaKind::Video, 500, 20, Some("A")),
        ];
        let plan_result = plan(&files, &HashMap::new(), &opts(true, false, true));
        assert_eq!(plan_result.merged.len(), 2);
        assert_eq!(plan_result.multicam, vec![MulticamGroup { angles: vec![vec![0], vec![1]], audio: Some(2) }]);
        assert_eq!(plan_result.unsynced, vec![3]);

        let no_mc = plan(&files, &HashMap::new(), &opts(true, false, false));
        assert!(no_mc.multicam.is_empty());
    }

    /// A video's scratch-track fingerprint and the matching production-audio fingerprint, the
    /// latter containing the same content `shift` samples later.
    fn waveform_pair(seed: u64, shift: i64) -> (Fingerprint, Fingerprint) {
        let base = synthetic_signal(8_000, seed);
        let video_fp = Fingerprint::from_samples(base.clone());
        let mut shifted = vec![0.0f32; base.len() + shift.unsigned_abs() as usize + 50];
        let offset = shift.max(0) as usize;
        for (i, &s) in base.iter().enumerate() {
            shifted[i + offset] = s;
        }
        (video_fp, Fingerprint::from_samples(shifted))
    }

    #[test]
    fn waveform_only_pairs_clips_with_no_timecode() {
        let mut files = vec![
            file("A001.mov", MediaKind::Video, 9_000, 10, Some("A")),
            file("1-001.wav", MediaKind::Audio, 0, 10, None),
        ];
        files[0].start = None; // camera clip never got jammed with timecode

        let (video_fp, audio_fp) = waveform_pair(7, 150);
        let mut fingerprints = HashMap::new();
        fingerprints.insert(files[0].path.clone(), video_fp);
        fingerprints.insert(files[1].path.clone(), audio_fp);

        let plan_result = plan(&files, &fingerprints, &opts(false, true, true));
        assert_eq!(plan_result.merged.len(), 1);
        let pair = &plan_result.merged[0];
        assert_eq!((pair.video, pair.audio), (0, 1));
        assert!(matches!(pair.method, SyncMethod::Waveform { .. }));

        let tick_per_sample = sample_ticks(8_000);
        let audio_pos = plan_result.positions[1].unwrap();
        let video_pos = plan_result.positions[0].unwrap();
        assert_eq!(audio_pos, 0); // no timecode anywhere: production audio anchors at 0
        assert!((video_pos - audio_pos - 150 * tick_per_sample).abs() <= tick_per_sample);
    }

    #[test]
    fn waveform_ignores_misleading_timecode_overlap() {
        // Two cameras overlap heavily by (wrong) timecode, but only A001 actually matches the WAV.
        let files = vec![
            file("A001.mov", MediaKind::Video, 100, 60, Some("A")),
            file("B001.mov", MediaKind::Video, 100, 60, Some("B")),
            file("1-001.wav", MediaKind::Audio, 9_000, 10, None),
        ];
        // The WAV's real timecode is nowhere near the cameras', so timecode pairing finds nothing.
        let fp_audio = synthetic_signal(8_000, 42);
        let mut fp_a = vec![0.0f32; fp_audio.len() + 100];
        for (i, &s) in fp_audio.iter().enumerate() {
            fp_a[i + 40] = s;
        }
        let mut fingerprints = HashMap::new();
        fingerprints.insert(files[0].path.clone(), Fingerprint::from_samples(fp_a));
        fingerprints.insert(files[2].path.clone(), Fingerprint::from_samples(fp_audio));
        // B001 has no usable audio at all, so it can never get a waveform match.

        let plan_result = plan(&files, &fingerprints, &opts(true, true, true));
        assert_eq!(plan_result.merged.len(), 1);
        assert_eq!(plan_result.merged[0].video, 0);
        assert!(plan_result.unsynced.contains(&1));
    }

    #[test]
    fn both_methods_keep_timecode_pairs_and_fill_in_the_rest() {
        let mut files = vec![
            file("A001.mov", MediaKind::Video, 100, 60, Some("A")), // timecode-paired below
            file("1-001.wav", MediaKind::Audio, 95, 70, None),
            file("B001.mov", MediaKind::Video, 9_000, 20, Some("B")), // no timecode, waveform-paired
        ];
        files[2].start = None;

        let (video_fp, audio_fp) = waveform_pair(99, 60);
        let mut fingerprints = HashMap::new();
        fingerprints.insert(files[2].path.clone(), video_fp);
        fingerprints.insert(files[1].path.clone(), audio_fp);

        let plan_result = plan(&files, &fingerprints, &opts(true, true, true));
        assert_eq!(plan_result.merged.len(), 2);
        let tc_pair = plan_result.merged.iter().find(|m| m.video == 0).unwrap();
        assert_eq!(tc_pair.audio, 1);
        assert!(matches!(tc_pair.method, SyncMethod::Timecode));
        let wf_pair = plan_result.merged.iter().find(|m| m.video == 2).unwrap();
        assert_eq!(wf_pair.audio, 1);
        assert!(matches!(wf_pair.method, SyncMethod::Waveform { .. }));
        // Both cameras land on the same (timecode) audio anchor, so they share one timeline.
        assert_eq!(plan_result.positions[1], files[1].start);
    }
}
