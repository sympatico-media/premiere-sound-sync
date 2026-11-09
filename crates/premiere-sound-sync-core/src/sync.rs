//! Timecode-based pairing of video with production audio, and multicam grouping.

use std::collections::{BTreeMap, BTreeSet};

use crate::media::{MediaFile, MediaKind};
use crate::time::TICKS_PER_SECOND;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedPair {
    pub video: usize,
    pub audio: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MulticamGroup {
    /// Angles: one entry per camera, each holding that camera's clips in time order.
    pub angles: Vec<Vec<usize>>,
    pub audio: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncPlan {
    pub merged: Vec<MergedPair>,
    pub multicam: Vec<MulticamGroup>,
    /// Files that didn't sync with anything; imported as plain clips.
    pub unsynced: Vec<usize>,
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

pub fn plan(files: &[MediaFile], multicam: bool) -> SyncPlan {
    let videos: Vec<usize> = (0..files.len())
        .filter(|&i| files[i].kind == MediaKind::Video)
        .collect();
    let audios: Vec<usize> = (0..files.len())
        .filter(|&i| files[i].kind == MediaKind::Audio)
        .collect();

    let mut merged = Vec::new();
    for &v in &videos {
        let best = audios
            .iter()
            .map(|&a| (a, overlap(&files[v], &files[a])))
            .filter(|&(a, ov)| is_meaningful(&files[v], &files[a], ov))
            .max_by_key(|&(_, ov)| ov);
        if let Some((a, _)) = best {
            merged.push(MergedPair { video: v, audio: a });
        }
    }

    let mut groups = Vec::new();
    if multicam {
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
                        ca.is_some() && cb.is_some() && ca != cb && is_meaningful(&files[a], &files[b], overlap(&files[a], &files[b]))
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
                angle.sort_by_key(|&v| files[v].start);
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::Slate;
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
        }
    }

    #[test]
    fn pairs_by_largest_overlap() {
        let files = vec![
            file("A001.mov", MediaKind::Video, 100, 60, Some("A")),
            file("1-001.wav", MediaKind::Audio, 95, 30, None),
            file("1-002.wav", MediaKind::Audio, 130, 60, None),
            file("lonely.wav", MediaKind::Audio, 9000, 10, None),
        ];
        let plan = plan(&files, true);
        assert_eq!(plan.merged, vec![MergedPair { video: 0, audio: 2 }]);
        assert!(plan.multicam.is_empty());
        assert_eq!(plan.unsynced, vec![1, 3]);
    }

    #[test]
    fn groups_cameras_sharing_a_take() {
        let files = vec![
            file("A001.mov", MediaKind::Video, 100, 60, Some("A")),
            file("B001.mov", MediaKind::Video, 102, 55, Some("B")),
            file("2-001.wav", MediaKind::Audio, 98, 70, None),
            file("A002.mov", MediaKind::Video, 500, 20, Some("A")),
        ];
        let plan = plan(&files, true);
        assert_eq!(plan.merged.len(), 2);
        assert_eq!(plan.multicam, vec![MulticamGroup { angles: vec![vec![0], vec![1]], audio: Some(2) }]);
        assert_eq!(plan.unsynced, vec![3]);

        let no_mc = super::plan(&files, false);
        assert!(no_mc.multicam.is_empty());
    }
}
