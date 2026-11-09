//! Finds audio and video files under folders.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::media::MediaKind;

const VIDEO_EXTENSIONS: &[&str] = &[
    "mov", "mp4", "m4v", "mxf", "avi", "mkv", "mts", "m2ts", "m2t", "ts", "mpg", "mpeg", "m2v", "3gp", "webm",
    "wmv", "flv", "dv", "r3d", "braw", "ari", "crm", "vob", "insv", "lrv",
];

const AUDIO_EXTENSIONS: &[&str] = &[
    "wav", "bwf", "rf64", "aif", "aiff", "aifc", "mp3", "m4a", "aac", "flac", "ogg", "oga", "opus", "wma", "caf",
    "ac3", "eac3", "amr",
];

pub fn classify(path: &Path) -> Option<MediaKind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if VIDEO_EXTENSIONS.contains(&ext.as_str()) {
        Some(MediaKind::Video)
    } else if AUDIO_EXTENSIONS.contains(&ext.as_str()) {
        Some(MediaKind::Audio)
    } else {
        None
    }
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.') || n.starts_with("._"))
}

/// Expands folders recursively and keeps loose files, returning sorted unique media paths.
pub fn collect(sources: &[PathBuf]) -> Vec<PathBuf> {
    let mut found = BTreeSet::new();
    for source in sources {
        if source.is_dir() {
            let walker = WalkDir::new(source)
                .follow_links(true)
                .into_iter()
                .filter_entry(|e| e.depth() == 0 || !is_hidden(e.path()));
            for entry in walker.flatten() {
                if entry.file_type().is_file() && classify(entry.path()).is_some() {
                    found.insert(entry.into_path());
                }
            }
        } else if source.is_file() && classify(source).is_some() && !is_hidden(source) {
            found.insert(source.clone());
        }
    }
    found.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_media_recursively() {
        let dir = std::env::temp_dir().join(format!("pss-scan-{}", std::process::id()));
        let nested = dir.join("Day 1/Sound");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(dir.join(".hidden")).unwrap();
        for f in ["A001.MOV", "notes.txt", "Day 1/Sound/2-001.WAV", ".hidden/x.wav", "Day 1/._A002.mov"] {
            std::fs::write(dir.join(f), b"").unwrap();
        }
        let files = collect(&[dir.clone(), dir.join("A001.MOV")]);
        let names: Vec<_> = files.iter().map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["A001.MOV", "Day 1/Sound/2-001.WAV"]);
    }
}
