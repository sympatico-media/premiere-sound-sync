//! Final Cut Pro 7 XML (`xmeml`) export, the interchange format Premiere and Resolve both import.
//!
//! Unlike `.prproj`, this is a plain, long-documented text format: there's no binary object
//! graph to guess at, so it's where every clip-layout idea lives, including ones (like
//! multicam-ready per-take sequences) that would be too risky to hand-author as Premiere's own
//! project format. A document holds one or more `<sequence>`s, each a run of takes laid end to
//! end: every camera angle on its own video track, each production audio channel on its own
//! audio track. Source files are listed once as master clips. Times are whole frames at the
//! timeline rate, so audio is placed to the nearest frame.

use std::collections::HashMap;
use std::fmt::Write;
use std::path::PathBuf;

use crate::build::DEFAULT_RATE;
use crate::media::{MediaFile, MediaKind};
use crate::prproj::objects::escape;
use crate::time::Rate;

pub const SOURCE_BIN: &str = "Source Media";

/// One take: its camera angle(s) and the production audio they're synced to, placed at each
/// file's position on the shared sync timeline (see [`crate::sync::SyncPlan`]).
pub struct Take<'a> {
    pub name: String,
    pub videos: Vec<&'a MediaFile>,
    pub video_positions: Vec<i64>,
    pub audio: &'a MediaFile,
    pub audio_position: i64,
}

/// Builds the XML document: `sequences` names each `<sequence>` and which takes (indices into
/// `takes`) it contains, in order.
pub fn export(takes: &[Take], sequences: &[(String, Vec<usize>)], trim: bool) -> String {
    let rate = takes.first().and_then(|t| t.videos.first().and_then(|v| v.rate)).unwrap_or(DEFAULT_RATE);

    let mut ids = Ids::default();
    let mut masters = String::new();
    for t in takes {
        for f in t.videos.iter().copied().chain(std::iter::once(t.audio)) {
            if !ids.files.contains_key(&f.path) {
                masters.push_str(&master_clip(&mut ids, f, rate));
            }
        }
    }

    let doc_name = sequences.first().map(|(n, _)| n.as_str()).unwrap_or("Synced");
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE xmeml>\n<xmeml version=\"4\">\n<project>\n");
    let _ = writeln!(out, "<name>{}</name>\n<children>", escape(doc_name));
    if !masters.is_empty() {
        let _ = writeln!(out, "<bin>\n<name>{SOURCE_BIN}</name>\n<children>\n{masters}</children>\n</bin>");
    }
    for (name, members) in sequences {
        let group: Vec<&Take> = members.iter().map(|&i| &takes[i]).collect();
        out.push_str(&timeline(&mut ids, name, rate, &group, trim));
    }
    out.push_str("</children>\n</project>\n</xmeml>\n");
    out
}

#[derive(Default)]
struct Ids {
    items: usize,
    /// Master clip id and file id for each source file.
    files: HashMap<PathBuf, (String, String)>,
}

impl Ids {
    fn item(&mut self) -> String {
        self.items += 1;
        format!("clipitem-{}", self.items)
    }
}

fn rate_xml(rate: Rate) -> String {
    let ntsc = if rate.den == 1001 { "TRUE" } else { "FALSE" };
    format!("<rate><timebase>{}</timebase><ntsc>{ntsc}</ntsc></rate>", rate.nominal_fps())
}

fn frames(ticks: i64, rate: Rate) -> i64 {
    ticks.div_euclid(rate.frame_ticks())
}

fn round_frames(ticks: i64, rate: Rate) -> i64 {
    let f = rate.frame_ticks();
    (ticks + f / 2).div_euclid(f)
}

/// `file://localhost/...` URL with everything outside the unreserved set percent-encoded.
fn path_url(path: &std::path::Path) -> String {
    let mut url = String::from("file://localhost");
    for b in path.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            url.push(b as char);
        } else {
            let _ = write!(url, "%{b:02X}");
        }
    }
    url
}

fn timecode_string(frame: i64, fps: i64, drop: bool) -> String {
    let mut n = frame.max(0);
    if drop {
        // Re-insert the frame numbers skipped by drop-frame counting.
        let dropped = if fps >= 50 { 4 } else { 2 };
        let per_ten = fps * 600 - dropped * 9;
        let per_min = fps * 60 - dropped;
        let tens = n / per_ten;
        let rem = n % per_ten;
        n += dropped * 9 * tens;
        if rem > dropped {
            n += dropped * ((rem - dropped) / per_min);
        }
    }
    let sep = if drop { ';' } else { ':' };
    let (ff, s) = (n % fps, n / fps);
    format!("{:02}:{:02}:{:02}{sep}{:02}", s / 3600 % 24, s / 60 % 60, s % 60, ff)
}

fn timecode_xml(frame: i64, rate: Rate, drop_frame: bool) -> String {
    let drop = drop_frame && rate.den == 1001;
    format!(
        "<timecode>{}<string>{}</string><frame>{frame}</frame><displayformat>{}</displayformat></timecode>",
        rate_xml(rate),
        timecode_string(frame, rate.nominal_fps(), drop),
        if drop { "DF" } else { "NDF" }
    )
}

/// Full description of a source file; written once, inside its master clip.
fn file_xml(id: &str, f: &MediaFile, rate: Rate) -> String {
    let mut x = format!(
        "<file id=\"{id}\"><name>{}</name><pathurl>{}</pathurl>{}<duration>{}</duration>",
        escape(&f.file_name()),
        path_url(&f.path),
        rate_xml(rate),
        frames(f.duration, rate)
    );
    if let Some(start) = f.start {
        x.push_str(&timecode_xml(round_frames(start, rate), rate, f.drop_frame));
    }
    x.push_str("<media>");
    if f.kind == MediaKind::Video {
        let _ = write!(
            x,
            "<video><samplecharacteristics>{}<width>{}</width><height>{}</height></samplecharacteristics></video>",
            rate_xml(rate),
            f.width.max(1),
            f.height.max(1)
        );
    }
    if f.audio_channels > 0 {
        let _ = write!(
            x,
            "<audio><samplecharacteristics><depth>24</depth><samplerate>{}</samplerate></samplecharacteristics><channelcount>{}</channelcount></audio>",
            f.sample_rate.max(1),
            f.audio_channels
        );
    }
    x.push_str("</media></file>");
    x
}

/// One clipitem on a track.
struct Item<'a> {
    id: String,
    name: &'a str,
    file: &'a MediaFile,
    /// `None` for video, otherwise the source audio channel.
    channel: Option<u32>,
    start: i64,
    end: i64,
    in_: i64,
    /// Track number (1-based): the camera angle index for video, the audio channel for audio.
    /// Also the index into the per-media-type track array, so track N holds `track - 1`.
    track: usize,
    /// Position on that track (1-based), for links.
    index: usize,
}

fn item_xml(ids: &Ids, rate: Rate, item: &Item, group: &[&Item], file: &str) -> String {
    let master = &ids.files[&item.file.path].0;
    let mut x = format!(
        "<clipitem id=\"{}\"><masterclipid>{master}</masterclipid><name>{}</name><enabled>TRUE</enabled><duration>{}</duration>{}<start>{}</start><end>{}</end><in>{}</in><out>{}</out>{file}",
        item.id,
        escape(item.name),
        frames(item.file.duration, rate),
        rate_xml(rate),
        item.start,
        item.end,
        item.in_,
        item.in_ + item.end - item.start
    );
    if let Some(channel) = item.channel {
        let _ = write!(
            x,
            "<sourcetrack><mediatype>audio</mediatype><trackindex>{}</trackindex></sourcetrack>",
            channel + 1
        );
    }
    for other in group {
        let media = if other.channel.is_some() { "audio" } else { "video" };
        let _ = write!(
            x,
            "<link><linkclipref>{}</linkclipref><mediatype>{media}</mediatype><trackindex>{}</trackindex><clipindex>{}</clipindex></link>",
            other.id, other.track, other.index
        );
    }
    x.push_str("</clipitem>");
    x
}

/// A master clip for `f` (all of its video and audio), defining the file for later references.
fn master_clip(ids: &mut Ids, f: &MediaFile, rate: Rate) -> String {
    let n = ids.files.len() + 1;
    let (master, file) = (format!("masterclip-{n}"), format!("file-{n}"));
    ids.files.insert(f.path.clone(), (master.clone(), file.clone()));
    let length = frames(f.duration, rate);
    let name = f.file_name();
    let channels: Vec<Option<u32>> = (f.kind == MediaKind::Video)
        .then_some(None)
        .into_iter()
        .chain((0..f.audio_channels).map(Some))
        .collect();
    let mut audio_track = 0;
    let items: Vec<Item> = channels
        .iter()
        .map(|&channel| Item {
            id: ids.item(),
            name: &name,
            file: f,
            channel,
            start: 0,
            end: length,
            in_: 0,
            track: match channel {
                None => 1,
                Some(_) => {
                    audio_track += 1;
                    audio_track
                }
            },
            index: 1,
        })
        .collect();
    let group: Vec<&Item> = items.iter().collect();
    let (mut video, mut audio) = (String::new(), String::new());
    for (i, item) in items.iter().enumerate() {
        let file_ref = if i == 0 { file_xml(&file, f, rate) } else { format!("<file id=\"{file}\"/>") };
        let track = format!("<track>{}</track>", item_xml(ids, rate, item, &group, &file_ref));
        if item.channel.is_some() { audio.push_str(&track) } else { video.push_str(&track) }
    }
    let mut x = format!(
        "<clip id=\"{master}\"><masterclipid>{master}</masterclipid><ismasterclip>TRUE</ismasterclip><name>{}</name><duration>{length}</duration>{}<in>-1</in><out>-1</out><media>",
        escape(&name),
        rate_xml(rate)
    );
    if !video.is_empty() {
        let _ = write!(x, "<video>{video}</video>");
    }
    if !audio.is_empty() {
        let _ = write!(x, "<audio>{audio}</audio>");
    }
    x.push_str("</media></clip>\n");
    x
}

/// The span (in frames, at `rate`) a take occupies: the outer edge of trimmed or untrimmed
/// video/audio, whichever `trim` asks for.
struct Span {
    zero: i64,
    end: i64,
    video_spans: Vec<(i64, i64)>,
    audio_span: (i64, i64),
}

fn span(take: &Take, rate: Rate, trim: bool) -> Span {
    let video_spans: Vec<(i64, i64)> = take
        .videos
        .iter()
        .zip(&take.video_positions)
        .map(|(v, &p)| {
            let s = round_frames(p, rate);
            (s, s + frames(v.duration, rate))
        })
        .collect();
    let a0 = round_frames(take.audio_position, rate);
    let audio_span = (a0, a0 + frames(take.audio.duration, rate));
    let starts = video_spans.iter().map(|&(s, _)| s).chain(std::iter::once(audio_span.0));
    let ends = video_spans.iter().map(|&(_, e)| e).chain(std::iter::once(audio_span.1));
    let (lo, hi) = (starts.clone().max().unwrap(), ends.clone().min().unwrap());
    let (zero, end) = if trim && lo < hi {
        (lo, hi)
    } else {
        (starts.min().unwrap(), ends.max().unwrap())
    };
    Span { zero, end, video_spans, audio_span }
}

/// A sequence: the given takes laid end to end, each one's angles and audio channels on their
/// own tracks (tracks are shared across takes, so angle 1 of every take lands on V1).
fn timeline(ids: &mut Ids, name: &str, rate: Rate, takes: &[&Take], trim: bool) -> String {
    let video_tracks = takes.iter().map(|t| t.videos.len()).max().unwrap_or(1).max(1);
    let audio_tracks = takes.iter().map(|t| t.audio.audio_channels.max(1) as usize).max().unwrap_or(1);
    let mut video: Vec<Vec<String>> = vec![Vec::new(); video_tracks];
    let mut audio: Vec<Vec<String>> = vec![Vec::new(); audio_tracks];
    let mut cursor = 0i64;
    for t in takes {
        let s = span(t, rate, trim);
        let base = cursor;
        let mut items = Vec::new();
        for (vi, &v) in t.videos.iter().enumerate() {
            let (s0, s1) = s.video_spans[vi];
            items.push(Item {
                id: ids.item(),
                name: &t.name,
                file: v,
                channel: None,
                start: base + s0.max(s.zero) - s.zero,
                end: base + s1.min(s.end) - s.zero,
                in_: s0.max(s.zero) - s0,
                track: vi + 1,
                index: video[vi].len() + 1,
            });
        }
        for ch in 0..t.audio.audio_channels.max(1) {
            let k = ch as usize;
            let (s0, s1) = s.audio_span;
            items.push(Item {
                id: ids.item(),
                name: &t.name,
                file: t.audio,
                channel: Some(ch),
                start: base + s0.max(s.zero) - s.zero,
                end: base + s1.min(s.end) - s.zero,
                in_: s0.max(s.zero) - s0,
                track: k + 1,
                index: audio[k].len() + 1,
            });
        }
        let group: Vec<&Item> = items.iter().collect();
        for item in &items {
            let file = format!("<file id=\"{}\"/>", ids.files[&item.file.path].1);
            let xml = item_xml(ids, rate, item, &group, &file);
            match item.channel {
                None => video[item.track - 1].push(xml),
                Some(_) => audio[item.track - 1].push(xml),
            }
        }
        cursor += s.end - s.zero;
    }
    let (width, height) = takes
        .first()
        .and_then(|t| t.videos.first())
        .map(|v| (v.width.max(1), v.height.max(1)))
        .unwrap_or((1920, 1080));
    let mut x = format!(
        "<sequence id=\"sequence-1\"><name>{}</name><duration>{cursor}</duration>{}{}<media><video><format><samplecharacteristics>{}<width>{width}</width><height>{height}</height></samplecharacteristics></format>",
        escape(name),
        rate_xml(rate),
        timecode_xml(0, rate, false),
        rate_xml(rate),
    );
    for track in &video {
        let _ = write!(x, "<track>{}</track>", track.concat());
    }
    x.push_str("</video><audio>");
    for track in &audio {
        let _ = write!(x, "<track>{}</track>", track.concat());
    }
    x.push_str("</audio></media></sequence>\n");
    x
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::Slate;

    const FRAME: i64 = 10_594_584_000;

    fn file(path: &str, kind: MediaKind, start: i64, frames: i64, channels: u32) -> MediaFile {
        MediaFile {
            path: path.into(),
            kind,
            start: Some(start * FRAME),
            duration: frames * FRAME,
            rate: (kind == MediaKind::Video).then(|| Rate::new(24000, 1001)),
            drop_frame: false,
            width: 3840,
            height: 2160,
            audio_channels: channels,
            sample_rate: 48000,
            channel_names: vec![],
            camera: None,
            reel: None,
            slate: Slate::default(),
            modified: None,
            recorded_at_source: None,
        }
    }

    fn take<'a>(name: &str, video: &'a MediaFile, audio: &'a MediaFile) -> Take<'a> {
        Take {
            name: name.into(),
            videos: vec![video],
            video_positions: vec![video.start.unwrap()],
            audio,
            audio_position: audio.start.unwrap(),
        }
    }

    #[test]
    fn synced_clips_are_laid_end_to_end_in_shoot_order() {
        // `export` places takes in the order it's given; it's the caller's job (normally
        // `build::group_takes`, which sorts by sync position) to hand them over in shoot order.
        let v1 = file("/m/A001.mov", MediaKind::Video, 1000, 200, 0);
        let a1 = file("/m/Sound Roll/2-001.WAV", MediaKind::Audio, 990, 230, 2);
        let v2 = file("/m/A002.mov", MediaKind::Video, 5000, 100, 0);
        let a2 = file("/m/Sound Roll/2-002.WAV", MediaKind::Audio, 5000, 100, 2);
        let takes = vec![take("2-001", &v1, &a1), take("2-002", &v2, &a2)];
        let seqs = vec![("Synced".to_string(), vec![0, 1])];

        let full = export(&takes, &seqs, false);
        assert!(full.contains("file://localhost/m/Sound%20Roll/2-001.WAV"));
        assert_eq!(full.matches("<pathurl>").count(), 4);
        assert_eq!(full.matches("<sequence ").count(), 1);
        // 2-001 was shot first: 230 frames with the video 10 frames in, then 2-002.
        assert!(full.contains("<name>Synced</name><duration>330</duration>"));
        assert!(full.contains("<name>2-001</name><enabled>TRUE</enabled><duration>200</duration><rate><timebase>24</timebase><ntsc>TRUE</ntsc></rate><start>10</start><end>210</end><in>0</in><out>200</out>"));
        assert!(full.contains("<start>230</start><end>330</end><in>0</in><out>100</out>"));

        let trimmed = export(&takes, &seqs, true);
        assert!(trimmed.contains("<name>Synced</name><duration>300</duration>"));
        assert!(trimmed.contains("<start>0</start><end>200</end><in>10</in><out>210</out>"));
    }

    #[test]
    fn multiple_sequences_share_one_source_bin() {
        let v1 = file("/m/A001.mov", MediaKind::Video, 1000, 100, 0);
        let a1 = file("/m/1-001.WAV", MediaKind::Audio, 1000, 100, 1);
        let v2 = file("/m/A002.mov", MediaKind::Video, 2000, 100, 0);
        let a2 = file("/m/1-002.WAV", MediaKind::Audio, 2000, 100, 1);
        let takes = vec![take("1-001", &v1, &a1), take("1-002", &v2, &a2)];
        let seqs = vec![
            ("All".to_string(), vec![0, 1]),
            ("1-001".to_string(), vec![0]),
            ("1-002".to_string(), vec![1]),
        ];
        let xml = export(&takes, &seqs, false);
        assert_eq!(xml.matches("<sequence ").count(), 3);
        assert_eq!(xml.matches("<pathurl>").count(), 4); // each source file described once
        assert_eq!(xml.matches("<bin>").count(), 1); // one shared Source Media bin
    }

    #[test]
    fn multicam_take_stacks_angles_on_separate_video_tracks() {
        let a = file("/m/A001.mov", MediaKind::Video, 1000, 100, 0);
        let b = file("/m/B001.mov", MediaKind::Video, 1002, 95, 0);
        let snd = file("/m/2-001.WAV", MediaKind::Audio, 998, 110, 1);
        let take = Take {
            name: "2-001".into(),
            videos: vec![&a, &b],
            video_positions: vec![a.start.unwrap(), b.start.unwrap()],
            audio: &snd,
            audio_position: snd.start.unwrap(),
        };
        let xml = export(&[take], &[("Synced".to_string(), vec![0])], false);
        assert_eq!(xml.matches("<video><format>").count(), 1); // one sequence
        assert_eq!(xml.matches("<track>").count(), 6); // 2 video + 1 audio master-clip tracks, + the sequence's 2 video + 1 audio
        assert_eq!(xml.matches("<link>").count(), 12); // 2 single-item master-clip video links + 3 sequence items, each linked to the other 2 (3*3=9)
    }

    #[test]
    fn drop_frame_timecode() {
        assert_eq!(timecode_string(1800, 30, true), "00:01:00;02");
        assert_eq!(timecode_string(17982, 30, true), "00:10:00;00");
        assert_eq!(timecode_string(86400, 24, false), "01:00:00:00");
    }
}
