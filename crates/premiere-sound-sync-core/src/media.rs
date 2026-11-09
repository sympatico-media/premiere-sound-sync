use std::path::PathBuf;
use std::time::SystemTime;

use crate::time::Rate;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaKind {
    Video,
    Audio,
}

/// Slate information: where a clip sits in the script.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Slate {
    pub scene: Option<String>,
    pub shot: Option<String>,
    pub take: Option<String>,
    pub circled: bool,
}

impl Slate {
    pub fn is_empty(&self) -> bool {
        self.scene.is_none() && self.shot.is_none() && self.take.is_none()
    }

    /// Fills missing fields from `other`.
    pub fn or(mut self, other: &Slate) -> Slate {
        if self.scene.is_none() {
            self.scene.clone_from(&other.scene);
        }
        if self.shot.is_none() {
            self.shot.clone_from(&other.shot);
        }
        if self.take.is_none() {
            self.take.clone_from(&other.take);
        }
        self.circled |= other.circled;
        self
    }
}

#[derive(Debug, Clone)]
pub struct MediaFile {
    pub path: PathBuf,
    pub kind: MediaKind,
    /// Timecode start in Premiere ticks, if the file carries timecode.
    pub start: Option<i64>,
    pub duration: i64,
    /// Video frame rate (video files only).
    pub rate: Option<Rate>,
    pub drop_frame: bool,
    pub width: u32,
    pub height: u32,
    pub audio_channels: u32,
    pub sample_rate: u32,
    /// Per-channel track names (e.g. `MixL`, `Boom`) when the recorder provides them.
    pub channel_names: Vec<String>,
    /// Camera / angle identifier used to tell multicam angles apart.
    pub camera: Option<String>,
    pub reel: Option<String>,
    pub slate: Slate,
    /// Filesystem modification time, used to narrow waveform-sync candidates on large shoots,
    /// and as the last-resort fallback for [`MediaFile::recorded`].
    pub modified: Option<SystemTime>,
    /// When the camera or recorder says this was actually recorded: ffprobe's `creation_time`
    /// tag, or a BWF file's bext `OriginationDate`/`OriginationTime`. `None` when neither is
    /// present (not every camera writes one); use [`MediaFile::recorded`] for a value that
    /// falls back to the filesystem modification time.
    pub recorded_at_source: Option<SystemTime>,
}

impl MediaFile {
    /// The best guess at when this was recorded: the camera/recorder's own timestamp if it
    /// wrote one, otherwise the file's filesystem modification time (close enough for grouping
    /// footage by shoot day on media that's gone straight from the card).
    pub fn recorded(&self) -> Option<SystemTime> {
        self.recorded_at_source.or(self.modified)
    }

    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn stem(&self) -> String {
        self.path
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn end(&self) -> Option<i64> {
        self.start.map(|s| s + self.duration)
    }

    pub fn channel_name(&self, index: usize) -> String {
        self.channel_names
            .get(index)
            .filter(|n| !n.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("Ch {}", index + 1))
    }
}
