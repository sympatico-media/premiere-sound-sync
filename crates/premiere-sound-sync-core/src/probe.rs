//! Builds [`MediaFile`]s: WAVs are read natively, everything else through ffprobe.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;

use crate::bwf;
use crate::media::{MediaFile, MediaKind, Slate};
use crate::naming;
use crate::scan;
use crate::time::{self, Rate, TICKS_PER_SECOND};

/// Locates ffprobe. GUI apps on macOS don't inherit the shell PATH, so common install dirs are checked too.
pub fn ffprobe_path() -> Option<PathBuf> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("ffprobe"));
                candidates.push(dir.join("../Resources/ffprobe"));
            }
        }
        if let Some(paths) = std::env::var_os("PATH") {
            candidates.extend(std::env::split_paths(&paths).map(|p| p.join("ffprobe")));
        }
        for dir in ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin", "/usr/bin"] {
            candidates.push(Path::new(dir).join("ffprobe"));
        }
        candidates.into_iter().find(|p| p.is_file())
    })
    .clone()
}

pub fn probe(path: &Path) -> Result<MediaFile> {
    let kind = scan::classify(path).ok_or_else(|| anyhow!("not a media file"))?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if kind == MediaKind::Audio && matches!(ext.as_str(), "wav" | "bwf" | "rf64") {
        if let Ok(file) = probe_wav(path) {
            return Ok(file);
        }
    }
    probe_ffprobe(path, kind)
}

fn probe_wav(path: &Path) -> Result<MediaFile> {
    let info = bwf::read_wav(path)?;
    let samples = info.total_samples() as i64;
    let tick = time::sample_ticks(info.sample_rate);
    let stem = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let slate = info.slate().or(&naming::slate_from_filename(&stem));
    Ok(MediaFile {
        path: path.to_path_buf(),
        kind: MediaKind::Audio,
        start: info.time_reference.map(|t| t as i64 * tick),
        duration: samples * tick,
        rate: None,
        drop_frame: false,
        width: 0,
        height: 0,
        audio_channels: info.channels,
        sample_rate: info.sample_rate,
        channel_names: info.channel_names(),
        camera: None,
        reel: None,
        slate,
    })
}

fn probe_ffprobe(path: &Path, kind: MediaKind) -> Result<MediaFile> {
    let ffprobe = ffprobe_path().ok_or_else(|| anyhow!("ffprobe not found (install FFmpeg)"))?;
    let output = Command::new(ffprobe)
        .args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams"])
        .arg(path)
        .output()
        .context("running ffprobe")?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    let json: Value = serde_json::from_slice(&output.stdout).context("parsing ffprobe output")?;
    from_ffprobe_json(path, kind, &json)
}

fn tag<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    let tags = v.get("tags")?.as_object()?;
    tags.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .and_then(|(_, v)| v.as_str())
        .filter(|s| !s.trim().is_empty())
}

fn num<T: std::str::FromStr>(v: &Value, key: &str) -> Option<T> {
    match v.get(key)? {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.to_string().parse().ok(),
        _ => None,
    }
}

pub(crate) fn from_ffprobe_json(path: &Path, kind: MediaKind, json: &Value) -> Result<MediaFile> {
    let streams = json.get("streams").and_then(Value::as_array).cloned().unwrap_or_default();
    let format = json.get("format").cloned().unwrap_or(Value::Null);
    let video = streams.iter().find(|s| {
        s.get("codec_type").and_then(Value::as_str) == Some("video")
            && s.get("disposition").and_then(|d| d.get("attached_pic")).and_then(Value::as_i64) != Some(1)
    });
    let audio: Vec<&Value> = streams
        .iter()
        .filter(|s| s.get("codec_type").and_then(Value::as_str) == Some("audio"))
        .collect();
    let kind = if video.is_none() { MediaKind::Audio } else { kind };
    if kind == MediaKind::Video && video.is_none() {
        bail!("no video stream");
    }
    if kind == MediaKind::Audio && audio.is_empty() {
        bail!("no audio stream");
    }

    let audio_channels: u32 = audio.iter().map(|a| num::<u32>(a, "channels").unwrap_or(1)).sum();
    let sample_rate = audio.first().and_then(|a| num::<u32>(a, "sample_rate")).unwrap_or(48_000);
    let seconds = video
        .and_then(|v| num::<f64>(v, "duration"))
        .or_else(|| num::<f64>(&format, "duration"))
        .unwrap_or(0.0);

    let timecode = streams
        .iter()
        .find_map(|s| tag(s, "timecode"))
        .or_else(|| tag(&format, "timecode"))
        .map(str::to_string);

    let stem = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let mut file = MediaFile {
        path: path.to_path_buf(),
        kind,
        start: None,
        duration: 0,
        rate: None,
        drop_frame: timecode.as_deref().is_some_and(time::is_drop_frame),
        width: 0,
        height: 0,
        audio_channels,
        sample_rate,
        channel_names: Vec::new(),
        camera: None,
        reel: None,
        slate: Slate::default(),
    };

    if let Some(v) = video {
        let rate = v
            .get("r_frame_rate")
            .and_then(Value::as_str)
            .and_then(Rate::parse)
            .filter(|r| r.fps() > 1.0 && r.fps() < 1000.0)
            .or_else(|| v.get("avg_frame_rate").and_then(Value::as_str).and_then(Rate::parse))
            .unwrap_or(Rate::new(24000, 1001));
        let frame = rate.frame_ticks();
        let frames = num::<i64>(v, "nb_frames")
            .filter(|n| *n > 0)
            .unwrap_or_else(|| (seconds * rate.fps()).round() as i64);
        file.rate = Some(rate);
        file.duration = frames * frame;
        file.width = num(v, "width").unwrap_or(1920);
        file.height = num(v, "height").unwrap_or(1080);
        file.start = timecode
            .as_deref()
            .and_then(|tc| time::timecode_to_frames(tc, rate))
            .map(|f| f * frame);
        file.camera = camera_id(&format, v, &stem);
        file.reel = tag(&format, "com.arri.camera.ReelName")
            .or_else(|| tag(&format, "reel_name"))
            .or_else(|| tag(v, "reel_name"))
            .map(str::to_string);
        let scene = tag(&format, "com.arri.camera.SceneName").or_else(|| tag(&format, "scene"));
        let take = tag(&format, "com.arri.camera.TakeName").or_else(|| tag(&format, "take"));
        let (scene, shot) = scene.map(naming::split_scene).unwrap_or((None, None));
        file.slate = Slate {
            scene,
            shot,
            take: take.map(str::to_string),
            circled: false,
        };
    } else {
        file.duration = (seconds * TICKS_PER_SECOND as f64).round() as i64;
        if let Some(tc) = &timecode {
            let rate = Rate::new(24000, 1001);
            file.start = time::timecode_to_frames(tc, rate).map(|f| f * rate.frame_ticks());
        }
        if let Some(tr) = tag(&format, "time_reference").and_then(|t| t.parse::<i64>().ok()) {
            file.start = Some(tr * time::sample_ticks(sample_rate));
        }
        if let Some(comment) = tag(&format, "comment") {
            let info = bwf::WavInfo {
                channels: audio_channels,
                description: comment.to_string(),
                ..Default::default()
            };
            file.slate = info.slate();
            file.channel_names = info.channel_names();
        }
    }
    file.slate = file.slate.clone().or(&naming::slate_from_filename(&stem));
    Ok(file)
}

fn camera_id(format: &Value, video: &Value, stem: &str) -> Option<String> {
    for key in [
        "com.arri.camera.CameraIndex",
        "com.apple.proapps.cameraID",
        "com.apple.proapps.cameraName",
        "camera_index",
        "com.sony.camera.id",
    ] {
        if let Some(v) = tag(format, key).or_else(|| tag(video, key)) {
            return Some(v.to_string());
        }
    }
    // Camera-original names like A001C003 / B002_C010 start with the camera letter.
    let first = stem.chars().next()?;
    let second = stem.chars().nth(1)?;
    (first.is_ascii_alphabetic() && second.is_ascii_digit()).then(|| first.to_ascii_uppercase().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_arri_like_ffprobe_output() {
        let json: Value = serde_json::from_str(
            r#"{
              "streams": [
                {"codec_type":"video","width":3200,"height":1800,"r_frame_rate":"24000/1001","nb_frames":"1486","duration":"61.978583"},
                {"codec_type":"audio","channels":5,"sample_rate":"48000"},
                {"codec_type":"data","tags":{"timecode":"02:57:11:17"}}
              ],
              "format": {"duration":"61.978583","tags":{"timecode":"02:57:11:17","com.arri.camera.CameraIndex":"A","com.arri.camera.ReelName":"A001C5F0"}}
            }"#,
        )
        .unwrap();
        let f = from_ffprobe_json(Path::new("/x/A001C001_260910_C5F0.mov"), MediaKind::Video, &json).unwrap();
        assert_eq!(f.start, Some(2_703_324_648_024_000));
        assert_eq!(f.duration, 15_743_551_824_000);
        assert_eq!(f.audio_channels, 5);
        assert_eq!(f.camera.as_deref(), Some("A"));
        assert_eq!(f.reel.as_deref(), Some("A001C5F0"));
    }
}
