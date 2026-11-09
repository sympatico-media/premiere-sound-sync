//! Final Cut Pro 7 XML (`xmeml`) export, the interchange format Premiere and Resolve both import.
//!
//! The output is one timeline of every synced clip, end to end in shoot order: video on V1 and
//! each production audio channel on its own track. Source files are listed once as master clips.
//! Times are whole frames at the timeline rate, so audio is placed to the nearest frame.

use std::collections::HashMap;
use std::fmt::Write;
use std::path::PathBuf;

use crate::build::{ItemKind, PlannedItem, DEFAULT_RATE};
use crate::media::{MediaFile, MediaKind};
use crate::prproj::objects::escape;
use crate::time::Rate;

pub const SOURCE_BIN: &str = "Source Media";

/// Builds the XML document: a timeline named `name` plus a bin of the source master clips.
pub fn export(files: &[MediaFile], items: &[PlannedItem], name: &str, trim: bool) -> String {
    let mut pairs: Vec<Pair> = items
        .iter()
        .filter_map(|item| match item.kind {
            ItemKind::Merged { video, audio } => Some(Pair::new(&item.name, &files[video], &files[audio], trim)),
            ItemKind::Plain(_) => None,
        })
        .collect();
    pairs.sort_by_key(|p| (p.zero, p.name.clone()));
    let rate = pairs.first().map(|p| p.rate).unwrap_or(DEFAULT_RATE);

    let mut ids = Ids::default();
    let mut masters = String::new();
    for p in &pairs {
        for f in [p.video, p.audio] {
            if !ids.files.contains_key(&f.path) {
                masters.push_str(&master_clip(&mut ids, f, rate));
            }
        }
    }

    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE xmeml>\n<xmeml version=\"4\">\n<project>\n");
    let _ = writeln!(out, "<name>{}</name>\n<children>", escape(name));
    if !masters.is_empty() {
        let _ = writeln!(out, "<bin>\n<name>{SOURCE_BIN}</name>\n<children>\n{masters}</children>\n</bin>");
    }
    out.push_str(&timeline(&mut ids, name, rate, &pairs));
    out.push_str("</children>\n</project>\n</xmeml>\n");
    out
}

/// A synced video and audio file, in absolute frames at the video rate.
struct Pair<'a> {
    name: String,
    video: &'a MediaFile,
    audio: &'a MediaFile,
    rate: Rate,
    zero: i64,
    end: i64,
    video_span: (i64, i64),
    audio_span: (i64, i64),
}

impl<'a> Pair<'a> {
    fn new(name: &str, video: &'a MediaFile, audio: &'a MediaFile, trim: bool) -> Self {
        let rate = video.rate.unwrap_or(DEFAULT_RATE);
        let v0 = round_frames(video.start.unwrap_or(0), rate);
        let a0 = round_frames(audio.start.unwrap_or(0), rate);
        let (v1, a1) = (v0 + frames(video.duration, rate), a0 + frames(audio.duration, rate));
        let (zero, end) = if trim && v0.max(a0) < v1.min(a1) {
            (v0.max(a0), v1.min(a1))
        } else {
            (v0.min(a0), v1.max(a1))
        };
        Pair {
            name: name.to_string(),
            video,
            audio,
            rate,
            zero,
            end,
            video_span: (v0, v1),
            audio_span: (a0, a1),
        }
    }

    fn channels(&self) -> u32 {
        self.audio.audio_channels.max(1)
    }
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
    /// Track number (1-based) and position on that track (1-based), for links.
    track: usize,
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

/// The timeline: pairs end to end, each pair's clipitems linked together.
fn timeline(ids: &mut Ids, name: &str, rate: Rate, pairs: &[Pair]) -> String {
    let audio_tracks = pairs.iter().map(Pair::channels).max().unwrap_or(1) as usize;
    let mut video: Vec<String> = Vec::new();
    let mut audio: Vec<Vec<String>> = vec![Vec::new(); audio_tracks];
    let mut cursor = 0;
    for p in pairs {
        let base = cursor;
        let place = |id: String, file, channel, (s0, s1): (i64, i64), track: usize, index: usize| Item {
            id,
            name: &p.name,
            file,
            channel,
            start: base + s0.max(p.zero) - p.zero,
            end: base + s1.min(p.end) - p.zero,
            in_: s0.max(p.zero) - s0,
            track,
            index,
        };
        let mut items = vec![place(ids.item(), p.video, None, p.video_span, 1, video.len() + 1)];
        for ch in 0..p.channels() {
            let k = ch as usize;
            items.push(place(ids.item(), p.audio, Some(ch), p.audio_span, k + 1, audio[k].len() + 1));
        }
        let group: Vec<&Item> = items.iter().collect();
        for item in &items {
            let file = format!("<file id=\"{}\"/>", ids.files[&item.file.path].1);
            let xml = item_xml(ids, rate, item, &group, &file);
            match item.channel {
                None => video.push(xml),
                Some(ch) => audio[ch as usize].push(xml),
            }
        }
        cursor += p.end - p.zero;
    }
    let (width, height) = pairs.first().map(|p| (p.video.width.max(1), p.video.height.max(1))).unwrap_or((1920, 1080));
    let mut x = format!(
        "<sequence id=\"sequence-1\"><name>{}</name><duration>{cursor}</duration>{}{}<media><video><format><samplecharacteristics>{}<width>{width}</width><height>{height}</height></samplecharacteristics></format><track>{}</track></video><audio>",
        escape(name),
        rate_xml(rate),
        timecode_xml(0, rate, false),
        rate_xml(rate),
        video.concat()
    );
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
        }
    }

    fn merged(name: &str, video: usize, audio: usize) -> PlannedItem {
        PlannedItem {
            name: name.into(),
            bin: vec![],
            kind: ItemKind::Merged { video, audio },
        }
    }

    #[test]
    fn synced_clips_are_laid_end_to_end_in_shoot_order() {
        let files = vec![
            file("/m/A002.mov", MediaKind::Video, 5000, 100, 0),
            file("/m/Sound Roll/2-002.WAV", MediaKind::Audio, 5000, 100, 2),
            file("/m/A001.mov", MediaKind::Video, 1000, 200, 0),
            file("/m/Sound Roll/2-001.WAV", MediaKind::Audio, 990, 230, 2),
        ];
        let items = vec![merged("2-002", 0, 1), merged("2-001", 2, 3)];

        let full = export(&files, &items, "Synced", false);
        assert!(full.contains("file://localhost/m/Sound%20Roll/2-001.WAV"));
        assert_eq!(full.matches("<pathurl>").count(), 4);
        assert_eq!(full.matches("<sequence ").count(), 1);
        // 2-001 was shot first: 230 frames with the video 10 frames in, then 2-002.
        assert!(full.contains("<name>Synced</name><duration>330</duration>"));
        assert!(full.contains("<name>2-001</name><enabled>TRUE</enabled><duration>200</duration><rate><timebase>24</timebase><ntsc>TRUE</ntsc></rate><start>10</start><end>210</end><in>0</in><out>200</out>"));
        assert!(full.contains("<start>230</start><end>330</end><in>0</in><out>100</out>"));

        let trimmed = export(&files, &items, "Synced", true);
        assert!(trimmed.contains("<name>Synced</name><duration>300</duration>"));
        assert!(trimmed.contains("<start>0</start><end>200</end><in>10</in><out>210</out>"));
    }

    #[test]
    fn drop_frame_timecode() {
        assert_eq!(timecode_string(1800, 30, true), "00:01:00;02");
        assert_eq!(timecode_string(17982, 30, true), "00:10:00;00");
        assert_eq!(timecode_string(86400, 24, false), "01:00:00:00");
    }
}
