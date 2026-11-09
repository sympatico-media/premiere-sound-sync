//! Generators for Premiere project objects.
//!
//! Structures mirror what Premiere Pro 2026 writes for imported clips and merged clips.

use crate::time::floor_to;

pub mod class {
    pub const CLIP_PROJECT_ITEM: &str = "cb4e0ed7-aca1-4171-8525-e3658dec06dd";
    pub const MASTER_CLIP: &str = "fb11c33a-b0a9-4465-aa94-b6d5db2628cf";
    pub const CLIP_LOGGING_INFO: &str = "77ab7fdd-dcdf-465d-9906-7a330ca1e738";
    pub const AUDIO_COMPONENT_CHAIN: &str = "3cb131d1-d3c0-47ae-a19a-bdf75ea11674";
    pub const VIDEO_COMPONENT_CHAIN: &str = "0970e08a-f58f-4108-b29a-1a717b8e12e2";
    pub const AUDIO_CLIP: &str = "b8830d03-de02-41ee-84ec-fe566dc70cd9";
    pub const VIDEO_CLIP: &str = "9308dbef-2440-4acb-9ab2-953b9a4e82ec";
    pub const AUDIO_SEQUENCE_SOURCE: &str = "e8d4cc83-38cb-491f-9d94-e5f7e3b205ee";
    pub const VIDEO_SEQUENCE_SOURCE: &str = "4752dfa9-7a7e-4a3b-a25b-cafde1a8d036";
    pub const SEQUENCE: &str = "6a15d903-8739-11d5-af2d-9b7855ad8974";
    pub const LINK: &str = "149d4ea5-a7d4-4b34-9bb7-16d783904bf2";
    pub const AUDIO_CLIP_TRACK_ITEM: &str = "064ec682-9ba6-11d5-af2d-9ca32c7d6164";
    pub const VIDEO_CLIP_TRACK_ITEM: &str = "368b0406-29e3-4923-9fcd-094fbf9a1089";
    pub const SUB_CLIP: &str = "e0c58dc9-dbdd-4166-aef7-5db7e3f22e84";
    pub const MARKERS: &str = "bee50706-b524-416c-9f03-b596ce5f6866";
    pub const AUDIO_MEDIA_SOURCE: &str = "f588da05-fc2a-4fbc-9383-74d653b379e3";
    pub const VIDEO_MEDIA_SOURCE: &str = "e64ddf74-8fac-4682-8aa8-0e0ca2248949";
    pub const MEDIA: &str = "7a5c103e-f3ac-4391-b6b4-7cc3d2f9a7ff";
    pub const AUDIO_STREAM: &str = "0b5cf52f-2b85-4863-890b-8844b64ecfe9";
    pub const VIDEO_STREAM: &str = "a36e4719-3ec6-4a0c-ab11-8b4aab377aa5";
    pub const SECONDARY_CONTENT: &str = "f9d004b5-cb04-4e2f-af6f-64fadc2c4be9";
    pub const CHANNEL_GROUPS: &str = "a3127a8c-95d4-456e-a7f5-171b3f922426";
    pub const CHANNEL_VECTOR: &str = "333d203b-3a53-4195-8894-fc7523ff3dc7";
    pub const CHANNEL: &str = "5c89aa7a-89a6-4483-becd-f2b1def42316";
    pub const VIDEO_TRACK_GROUP: &str = "9e9abf7a-0918-49c2-91ae-991b5dde77bb";
    pub const VIDEO_CLIP_TRACK: &str = "f68dcd81-8805-11d5-af2d-9bfa89d4ddd4";
    pub const AUDIO_TRACK_GROUP: &str = "9b9238b9-53a8-4cc3-b03f-b36246d052e6";
    pub const AUDIO_CLIP_TRACK: &str = "097f6203-99ae-11d5-84f2-8cf14bde7040";
    pub const AUDIO_MIX_TRACK: &str = "4b1d8400-e89e-11d5-abc4-a1a13b1e80a0";
    pub const AUDIO_TRACK_INLET: &str = "be3af080-e8c6-11d5-abc4-a1c6d5dee670";
    pub const DATA_TRACK_GROUP: &str = "b714b71d-6838-48dd-9b77-db19088ced7e";
    pub const AUDIO_FADER: &str = "1a38c583-ed5c-11d5-abc4-c1cbf61ec590";
    pub const PARAM_VOLUME: &str = "a714635e-a628-4b27-9d59-77eba47dbc1a";
    pub const PARAM_MUTE: &str = "32657501-3aa4-445f-a49b-d09ecb9fa1ae";
    pub const AUDIO_METER: &str = "72ea4700-f615-11d5-abc4-c186585e63e0";
    pub const DEFAULT_PAN: &str = "33a94282-ee2c-11d5-abc4-c1cd7f9e3c10";
}

pub const IMPLEMENTATION_ID: &str = "1fa18bfa-255c-44b1-ad73-56bcd99fceaf";
pub const MEDIA_TYPE_VIDEO: &str = "228cda18-3625-4d2d-951e-348879e4ed93";
pub const MEDIA_TYPE_AUDIO: &str = "80b8e3d5-6dca-4195-aefb-cb5f407ab009";
pub const MEDIA_TYPE_DATA: &str = "d8143ffe-eec4-4d2a-a909-d5f7bf094dc5";
const MONO: &str = r#"[{"channellabel":0}]"#;
const NO_CHANNEL: &str = "18446744073709551615";

pub fn new_uid() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub use super::xml::escape;

/// Accumulates new top-level objects and hands out unique `ObjectID`s.
pub struct Writer {
    next_id: u64,
    objects: Vec<String>,
}

impl Writer {
    pub fn new(first_id: u64) -> Self {
        Self {
            next_id: first_id,
            objects: Vec::new(),
        }
    }

    pub fn objects(&self) -> &[String] {
        &self.objects
    }

    fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn push(&mut self, s: String) {
        self.objects.push(s);
    }

    fn obj(&mut self, tag: &str, class: &str, version: u32, body: &str) -> u64 {
        let id = self.id();
        self.push(format!(r#"<{tag} ObjectID="{id}" ClassID="{class}" Version="{version}">{body}</{tag}>"#));
        id
    }

    fn uobj(&mut self, tag: &str, class: &str, version: u32, body: &str) -> String {
        let uid = new_uid();
        self.push(format!(r#"<{tag} ObjectUID="{uid}" ClassID="{class}" Version="{version}">{body}</{tag}>"#));
        uid
    }
}

/// A video file as Premiere sees it.
#[derive(Debug, Clone)]
pub struct VideoSpec {
    pub path: String,
    pub title: String,
    pub start: i64,
    pub duration: i64,
    pub frame: i64,
    pub time_display: u32,
    pub width: u32,
    pub height: u32,
    /// Camera audio channels to import alongside the picture (0 for none).
    pub audio_channels: u32,
    pub sample_tick: i64,
}

/// An audio file; every channel becomes its own mono stream, like Premiere's default mapping.
#[derive(Debug, Clone)]
pub struct AudioSpec {
    pub path: String,
    pub title: String,
    pub start: i64,
    pub duration: i64,
    pub sample_tick: i64,
    pub channels: u32,
    pub channel_names: Vec<String>,
}

impl AudioSpec {
    fn channel_name(&self, i: usize) -> String {
        self.channel_names
            .get(i)
            .filter(|n| !n.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("Ch {}", i + 1))
    }
}

#[derive(Debug, Clone, Copy)]
struct Chain {
    source: u64,
    markers: u64,
}

fn markers(w: &mut Writer) -> u64 {
    w.obj(
        "Markers",
        class::MARKERS,
        4,
        "<ByGUID>byGUID</ByGUID><LastMetadataState>00000000-0000-0000-0000-000000000000</LastMetadataState><LastContentState>00000000-0000-0000-0000-000000000000</LastContentState>",
    )
}

fn media_paths(path: &str, title: &str) -> String {
    let p = escape(path);
    format!(
        "<ActualMediaFilePath>{p}</ActualMediaFilePath><FilePath>{p}</FilePath><Title>{}</Title>",
        escape(title)
    )
}

fn audio_stream(w: &mut Writer, duration: i64, sample_tick: i64) -> u64 {
    w.obj(
        "AudioStream",
        class::AUDIO_STREAM,
        8,
        &format!("<AudioChannelLayout>{MONO}</AudioChannelLayout><Duration>{duration}</Duration><FrameRate>{sample_tick}</FrameRate>"),
    )
}

fn audio_media_source(w: &mut Writer, media_uid: &str, duration: i64) -> u64 {
    w.obj(
        "AudioMediaSource",
        class::AUDIO_MEDIA_SOURCE,
        2,
        &format!(r#"<MediaSource Version="4"><Content Version="10"></Content><Media ObjectURef="{media_uid}"/></MediaSource><OriginalDuration>{duration}</OriginalDuration>"#),
    )
}

/// One mono channel of an audio file: stream, media, source and markers.
fn audio_channel(w: &mut Writer, a: &AudioSpec, channel: u32, time_display: u32) -> Chain {
    let stream = audio_stream(w, a.duration, a.sample_tick);
    let stream_number = if channel > 0 {
        format!("<StreamNumber>{channel}</StreamNumber>")
    } else {
        String::new()
    };
    let media = w.uobj(
        "Media",
        class::MEDIA,
        30,
        &format!(
            r#"<AudioStream ObjectRef="{stream}"/><Start>{}</Start><ImplementationID>{IMPLEMENTATION_ID}</ImplementationID><FileKey>{}</FileKey><ConformedAudioRate>{}</ConformedAudioRate>{stream_number}<TimeDisplay>{time_display}</TimeDisplay>{}"#,
            a.start,
            new_uid(),
            a.sample_tick,
            media_paths(&a.path, &a.title)
        ),
    );
    let source = audio_media_source(w, &media, a.duration);
    let markers = markers(w);
    Chain { source, markers }
}

/// Video media, plus one chain per camera audio channel (channel 0 shares the video's Media).
fn video_media(w: &mut Writer, v: &VideoSpec) -> (Chain, Vec<Chain>) {
    let vstream = w.obj(
        "VideoStream",
        class::VIDEO_STREAM,
        23,
        &format!(
            "<Duration>{}</Duration><FrameRate>{}</FrameRate><FrameRect>0,0,{},{}</FrameRect>",
            v.duration, v.frame, v.width, v.height
        ),
    );
    let first_audio = (v.audio_channels > 0).then(|| audio_stream(w, v.duration, v.sample_tick));
    let audio_ref = first_audio
        .map(|id| format!(r#"<AudioStream ObjectRef="{id}"/>"#))
        .unwrap_or_default();
    let conformed = if v.audio_channels > 0 {
        format!("<ConformedAudioRate>{}</ConformedAudioRate>", v.sample_tick)
    } else {
        String::new()
    };
    let media = w.uobj(
        "Media",
        class::MEDIA,
        30,
        &format!(
            r#"{audio_ref}<VideoStream ObjectRef="{vstream}"/><Start>{}</Start><ImplementationID>{IMPLEMENTATION_ID}</ImplementationID><FileKey>{}</FileKey>{conformed}<TimeDisplay>{}</TimeDisplay>{}"#,
            v.start,
            new_uid(),
            v.time_display,
            media_paths(&v.path, &v.title)
        ),
    );
    let vsource = w.obj(
        "VideoMediaSource",
        class::VIDEO_MEDIA_SOURCE,
        2,
        &format!(r#"<MediaSource Version="4"><Content Version="10"></Content><Media ObjectURef="{media}"/></MediaSource><OriginalDuration>{}</OriginalDuration>"#, v.duration),
    );
    let vmarkers = markers(w);

    let mut audio = Vec::new();
    if v.audio_channels > 0 {
        let source = audio_media_source(w, &media, v.duration);
        audio.push(Chain { source, markers: vmarkers });
        let spec = AudioSpec {
            path: v.path.clone(),
            title: v.title.clone(),
            start: v.start,
            duration: v.duration,
            sample_tick: v.sample_tick,
            channels: v.audio_channels,
            channel_names: vec![],
        };
        for ch in 1..v.audio_channels {
            let mut chain = audio_channel(w, &spec, ch, v.time_display);
            chain.markers = vmarkers;
            audio.push(chain);
        }
    }
    (
        Chain {
            source: vsource,
            markers: vmarkers,
        },
        audio,
    )
}

fn default_audio_chain(w: &mut Writer) -> u64 {
    w.obj(
        "AudioComponentChain",
        class::AUDIO_COMPONENT_CHAIN,
        4,
        r#"<DefaultVol>true</DefaultVol><DefaultVolumeComponentID>1</DefaultVolumeComponentID><ComponentChain Version="3"></ComponentChain>"#,
    )
}

fn in_out(range: Option<(i64, i64)>) -> String {
    range
        .map(|(i, o)| format!("<InPoint>{i}</InPoint><OutPoint>{o}</OutPoint>"))
        .unwrap_or_default()
}

fn marker_owner(markers: Option<u64>) -> String {
    markers
        .map(|m| format!(r#"<MarkerOwner Version="1"><Markers ObjectRef="{m}"/></MarkerOwner>"#))
        .unwrap_or_default()
}

fn audio_clip(w: &mut Writer, source: u64, markers: Option<u64>, range: Option<(i64, i64)>, not_in_use: bool, gain: bool) -> u64 {
    let secondary = w.obj(
        "SecondaryContent",
        class::SECONDARY_CONTENT,
        1,
        &format!(r#"<Content ObjectRef="{source}"/><ChannelIndex>0</ChannelIndex>"#),
    );
    let in_use = if not_in_use { "<InUse>false</InUse>" } else { "" };
    let gain = if gain { "<Gain>1</Gain>" } else { "" };
    w.obj(
        "AudioClip",
        class::AUDIO_CLIP,
        8,
        &format!(
            r#"<Clip Version="18">{}<Source ObjectRef="{source}"/><ClipID>{}</ClipID>{in_use}{}</Clip><SecondaryContents Version="1"><SecondaryContentItem Index="0" ObjectRef="{secondary}"/></SecondaryContents><AudioChannelLayout>{MONO}</AudioChannelLayout>{gain}"#,
            marker_owner(markers),
            new_uid(),
            in_out(range)
        ),
    )
}

fn video_clip(w: &mut Writer, source: u64, markers: Option<u64>, range: Option<(i64, i64)>, not_in_use: bool, poster: bool) -> u64 {
    let in_use = if not_in_use { "<InUse>false</InUse>" } else { "" };
    let poster = if poster { "<PosterFrame>0</PosterFrame>" } else { "" };
    w.obj(
        "VideoClip",
        class::VIDEO_CLIP,
        11,
        &format!(
            r#"<Clip Version="18">{}<Source ObjectRef="{source}"/><ClipID>{}</ClipID>{in_use}{}</Clip>{poster}"#,
            marker_owner(markers),
            new_uid(),
            in_out(range)
        ),
    )
}

enum Groups {
    /// Channel j of the clip plays source clip j (normal multichannel mapping).
    All(u32),
    /// Only channel `k` is enabled (merged clip component masters).
    Only(u32, u32),
    Empty,
}

fn channel_groups(w: &mut Writer, groups: Groups) -> u64 {
    let entries: Vec<Option<u32>> = match groups {
        Groups::All(n) => (0..n).map(Some).collect(),
        Groups::Only(n, k) => (0..n).map(|j| (j == k).then_some(j)).collect(),
        Groups::Empty => vec![],
    };
    if entries.is_empty() {
        return w.obj("ClipChannelGroupVectorSerializer", class::CHANNEL_GROUPS, 1, "");
    }
    let mut vectors = Vec::new();
    for e in entries {
        let (ch, clip) = match e {
            Some(j) => ("0".to_string(), j.to_string()),
            None => (NO_CHANNEL.to_string(), NO_CHANNEL.to_string()),
        };
        let c = w.obj(
            "ClipChannelSerializer",
            class::CHANNEL,
            1,
            &format!("<mSourceChannelIndex>{ch}</mSourceChannelIndex><SourceClipIndex>{clip}</SourceClipIndex>"),
        );
        let v = w.obj(
            "ClipChannelVectorSerializer",
            class::CHANNEL_VECTOR,
            1,
            &format!(r#"<ClipChannels Version="1"><ClipChannelItem Index="0" ObjectRef="{c}"/></ClipChannels><ChannelType>0</ChannelType>"#),
        );
        vectors.push(v);
    }
    let items: String = vectors
        .iter()
        .enumerate()
        .map(|(i, v)| format!(r#"<ClipChannelVectorItem Index="{i}" ObjectRef="{v}"/>"#))
        .collect();
    w.obj(
        "ClipChannelGroupVectorSerializer",
        class::CHANNEL_GROUPS,
        1,
        &format!(r#"<ClipChannelVectors Version="1">{items}</ClipChannelVectors>"#),
    )
}

struct Logging<'a> {
    clip_name: &'a str,
    capture_mode: bool,
    media_in: i64,
    media_out: i64,
    rate: i64,
    format: u32,
}

fn logging(w: &mut Writer, l: Logging) -> u64 {
    let capture = if l.capture_mode { "<CaptureMode>1</CaptureMode>" } else { "" };
    w.obj(
        "ClipLoggingInfo",
        class::CLIP_LOGGING_INFO,
        10,
        &format!(
            "{capture}<ClipName>{}</ClipName><MediaInPoint>{}</MediaInPoint><MediaOutPoint>{}</MediaOutPoint><MediaFrameRate>{}</MediaFrameRate><TimecodeFormat>{}</TimecodeFormat>",
            escape(l.clip_name),
            l.media_in,
            l.media_out,
            l.rate,
            l.format
        ),
    )
}

struct Master<'a> {
    properties: &'a [(&'a str, String)],
    logging: u64,
    audio_chains: usize,
    clips: &'a [u64],
    groups: u64,
    name: &'a str,
}

fn master_clip(w: &mut Writer, m: Master) -> String {
    let props: String = m
        .properties
        .iter()
        .map(|(k, v)| format!("<{k}>{}</{k}>", escape(v)))
        .collect();
    let node = if props.is_empty() {
        String::new()
    } else {
        format!(r#"<Node Version="1"><Properties Version="1">{props}</Properties></Node>"#)
    };
    let chains: Vec<u64> = (0..m.audio_chains).map(|_| default_audio_chain(w)).collect();
    let chains_xml = if chains.is_empty() {
        String::new()
    } else {
        let items: String = chains
            .iter()
            .enumerate()
            .map(|(i, c)| format!(r#"<AudioComponentChain Index="{i}" ObjectRef="{c}"/>"#))
            .collect();
        format!(r#"<AudioComponentChains Version="1">{items}</AudioComponentChains>"#)
    };
    let clips: String = m
        .clips
        .iter()
        .enumerate()
        .map(|(i, c)| format!(r#"<Clip Index="{i}" ObjectRef="{c}"/>"#))
        .collect();
    w.uobj(
        "MasterClip",
        class::MASTER_CLIP,
        12,
        &format!(
            r#"{node}<LoggingInfo ObjectRef="{}"/>{chains_xml}<Clips Version="1">{clips}</Clips><AudioClipChannelGroups ObjectRef="{}"/><Name>{}</Name><MasterClipChangeVersion>1</MasterClipChangeVersion>"#,
            m.logging,
            m.groups,
            escape(m.name)
        ),
    )
}

fn clip_project_item(w: &mut Writer, name: &str, master: &str) -> String {
    w.uobj(
        "ClipProjectItem",
        class::CLIP_PROJECT_ITEM,
        1,
        &format!(
            r#"<ProjectItem Version="1"><Node Version="1"><Properties Version="1"></Properties></Node><Name>{}</Name></ProjectItem><MasterClip ObjectURef="{master}"/>"#,
            escape(name)
        ),
    )
}

/// Imports a video file (with its camera audio) as a regular clip. Returns the project item UID.
pub fn plain_video_clip(w: &mut Writer, name: &str, v: &VideoSpec) -> String {
    let (video, audio) = video_media(w, v);
    let mut clips = vec![video_clip(w, video.source, Some(video.markers), None, false, true)];
    for chain in &audio {
        clips.push(audio_clip(w, chain.source, Some(chain.markers), None, false, true));
    }
    let log = logging(
        w,
        Logging {
            clip_name: &v.title,
            capture_mode: false,
            media_in: v.start,
            media_out: v.start + v.duration,
            rate: v.frame,
            format: v.time_display,
        },
    );
    let groups = if audio.is_empty() {
        channel_groups(w, Groups::Empty)
    } else {
        channel_groups(w, Groups::All(audio.len() as u32))
    };
    let master = master_clip(
        w,
        Master {
            properties: &[],
            logging: log,
            audio_chains: audio.len(),
            clips: &clips,
            groups,
            name,
        },
    );
    clip_project_item(w, name, &master)
}

/// Imports an audio file as a regular multichannel clip. Returns the project item UID.
pub fn plain_audio_clip(w: &mut Writer, name: &str, a: &AudioSpec, time_display: u32) -> String {
    let chains: Vec<Chain> = (0..a.channels).map(|ch| audio_channel(w, a, ch, time_display)).collect();
    let clips: Vec<u64> = chains
        .iter()
        .map(|c| audio_clip(w, c.source, Some(c.markers), None, false, true))
        .collect();
    let log = logging(
        w,
        Logging {
            clip_name: &a.title,
            capture_mode: false,
            media_in: a.start,
            media_out: a.start + a.duration,
            rate: a.sample_tick,
            format: 200,
        },
    );
    let groups = channel_groups(w, Groups::All(a.channels));
    let master = master_clip(
        w,
        Master {
            properties: &[],
            logging: log,
            audio_chains: chains.len(),
            clips: &clips,
            groups,
            name,
        },
    );
    clip_project_item(w, name, &master)
}

/// Sample-accurate placement of the video and audio inside a merged clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergeTiming {
    pub zero_point: i64,
    pub audio_in: i64,
    pub audio_out: i64,
    pub audio_start: i64,
    pub audio_end: i64,
    pub video_start: i64,
    pub video_end: i64,
    /// Source range of the video, relative to the start of the file.
    pub video_in: i64,
    pub video_out: i64,
    /// Source in point of the audio track items, relative to the start of the file.
    pub audio_item_in: i64,
    pub duration: i64,
}

/// Merged clip timing trimmed to the frames where both video and audio exist.
/// Returns `None` when the two don't overlap by at least one frame.
pub fn trimmed_merge_timing(video_start: i64, video_duration: i64, frame: i64, audio_start: i64, audio_duration: i64) -> Option<MergeTiming> {
    let lo = video_start.max(audio_start);
    let hi = (video_start + video_duration).min(audio_start + audio_duration);
    let start = -floor_to(-lo, frame);
    let end = floor_to(hi, frame);
    if end <= start {
        return None;
    }
    let length = end - start;
    Some(MergeTiming {
        zero_point: start,
        audio_in: start - audio_start,
        audio_out: end - audio_start,
        audio_start: 0,
        audio_end: length,
        video_start: 0,
        video_end: length,
        video_in: start - video_start,
        video_out: end - video_start,
        audio_item_in: start - audio_start,
        duration: length,
    })
}

/// Computes merged clip timing the way Premiere does, everything frame-aligned to the video.
pub fn merge_timing(video_start: i64, video_duration: i64, frame: i64, audio_start: i64, audio_duration: i64) -> MergeTiming {
    let zero_point = floor_to(video_start.min(audio_start), frame);
    let audio_frame_start = floor_to(audio_start, frame);
    let audio_frames = (audio_start - audio_frame_start + audio_duration).div_euclid(frame);
    let audio_in = zero_point - audio_start;
    let audio_out = audio_in + audio_frames * frame;
    let audio_track_start = audio_frame_start - zero_point;
    let audio_track_end = audio_track_start + (audio_frames - 1) * frame;
    let video_track_start = video_start - zero_point;
    let video_track_end = video_track_start + floor_to(video_duration, frame);
    MergeTiming {
        zero_point,
        audio_in,
        audio_out,
        audio_start: audio_track_start,
        audio_end: audio_track_end,
        video_start: video_track_start,
        video_end: video_track_end,
        video_in: 0,
        video_out: video_duration,
        audio_item_in: 0,
        duration: audio_track_end.max(video_track_end),
    }
}

fn audio_track_components(w: &mut Writer, sample_tick: i64) -> (u64, u64) {
    let volume = w.obj(
        "AudioComponentParam",
        class::PARAM_VOLUME,
        10,
        "<Name>Volume</Name><RangeLocked>false</RangeLocked><UnitsString>dB</UnitsString><UpperBound>5.6234130859375</UpperBound>",
    );
    let mute = w.obj("AudioComponentParam", class::PARAM_MUTE, 10, "<Name>Mute</Name><RangeLocked>false</RangeLocked>");
    let audio_component = |kind: u32| {
        format!(
            r#"<AudioChannelLayout>{MONO}</AudioChannelLayout><AudioComponentType>{kind}</AudioComponentType><FrameRate>{sample_tick}</FrameRate><ChannelType>0</ChannelType>"#
        )
    };
    let fader = w.obj(
        "AudioFader",
        class::AUDIO_FADER,
        3,
        &format!(
            r#"<AudioComponent Version="3"><Component Version="7"><Params Version="1"><Param Index="0" ObjectRef="{volume}"/><Param Index="1" ObjectRef="{mute}"/></Params><ID>1</ID></Component>{}</AudioComponent>"#,
            audio_component(1)
        ),
    );
    let meter = w.obj(
        "AudioMeter",
        class::AUDIO_METER,
        2,
        &format!(r#"<AudioComponent Version="3"><Component Version="7"><ID>2</ID></Component>{}</AudioComponent>"#, audio_component(2)),
    );
    let chain = w.obj(
        "AudioComponentChain",
        class::AUDIO_COMPONENT_CHAIN,
        4,
        &format!(r#"<ComponentChain Version="3"><Components Version="1"><Component Index="0" ObjectRef="{fader}"/><Component Index="1" ObjectRef="{meter}"/></Components></ComponentChain>"#),
    );
    let panner = w.obj(
        "DefaultPanProcessor",
        class::DEFAULT_PAN,
        2,
        &format!(
            r#"<PanProcessor Version="3"><AudioComponent Version="3"><Component Version="7"><ID>4294967280</ID></Component>{}</AudioComponent></PanProcessor><DefaultPannerOutputChannelType>0</DefaultPannerOutputChannelType><DefaultPannerInputChannelType>0</DefaultPannerInputChannelType>"#,
            audio_component(0)
        ),
    );
    (chain, panner)
}

fn track_header(id: u32, media_type: &str, index: usize, targeted: bool) -> String {
    let node = if targeted {
        r#"<Node Version="1"><Properties Version="1"><MZ.TrackTargeted>1</MZ.TrackTargeted></Properties></Node>"#
    } else {
        ""
    };
    format!(r#"<Track Version="4">{node}<ID>{id}</ID><MediaType>{media_type}</MediaType><Index>{index}</Index></Track>"#)
}

fn clip_items(items: &[u64], media_type: &str, index: usize) -> String {
    let list: String = items
        .iter()
        .enumerate()
        .map(|(i, t)| format!(r#"<TrackItem Index="{i}" ObjectRef="{t}"/>"#))
        .collect();
    let track_items = if list.is_empty() {
        String::new()
    } else {
        format!(r#"<TrackItems Version="1">{list}</TrackItems>"#)
    };
    format!(
        r#"<ClipItems Version="3">{track_items}<MediaType>{media_type}</MediaType><Index>{index}</Index></ClipItems><TransitionItems Version="3"><MediaType>{media_type}</MediaType><Index>{index}</Index></TransitionItems>"#
    )
}

fn audio_clip_track(w: &mut Writer, id: u32, index: usize, items: &[u64], sample_tick: i64) -> String {
    let (chain, panner) = audio_track_components(w, sample_tick);
    w.uobj(
        "AudioClipTrack",
        class::AUDIO_CLIP_TRACK,
        7,
        &format!(
            r#"<ClipTrack Version="2">{}{}</ClipTrack><AudioTrack Version="12"><ComponentOwner Version="1"><Components ObjectRef="{chain}"/></ComponentOwner><Panner ObjectRef="{panner}"/><ID>{}</ID><NextPannerID>4294967279</NextPannerID><ChannelType>0</ChannelType></AudioTrack>"#,
            track_header(id, MEDIA_TYPE_AUDIO, index, false),
            clip_items(items, MEDIA_TYPE_AUDIO, index),
            new_uid()
        ),
    )
}

fn video_clip_track(w: &mut Writer, id: u32, index: usize, items: &[u64]) -> String {
    w.uobj(
        "VideoClipTrack",
        class::VIDEO_CLIP_TRACK,
        1,
        &format!(
            r#"<ClipTrack Version="2">{}{}</ClipTrack>"#,
            track_header(id, MEDIA_TYPE_VIDEO, index, index == 0),
            clip_items(items, MEDIA_TYPE_VIDEO, index)
        ),
    )
}

pub struct TrackGroups {
    pub video: u64,
    pub audio: u64,
    pub data: u64,
}

/// Builds the video, audio and data track groups of a sequence.
fn track_groups(w: &mut Writer, video_tracks: &[String], audio_tracks: &[String], frame: i64, sample_tick: i64, width: u32, height: u32) -> TrackGroups {
    let video_chain = w.obj("VideoComponentChain", class::VIDEO_COMPONENT_CHAIN, 3, r#"<ComponentChain Version="3"></ComponentChain>"#);
    let vtracks: String = video_tracks
        .iter()
        .enumerate()
        .map(|(i, t)| format!(r#"<Track Index="{i}" ObjectURef="{t}"/>"#))
        .collect();
    let video = w.obj(
        "VideoTrackGroup",
        class::VIDEO_TRACK_GROUP,
        13,
        &format!(
            r#"<TrackGroup Version="1"><Tracks Version="1">{vtracks}</Tracks><NextTrackID>{}</NextTrackID><FrameRate>{frame}</FrameRate></TrackGroup><ColorManagementSettings>{{"autoToneMapEnabled":true,"enableLogColorManagement":2,"lutInterpolationMethod":1}}</ColorManagementSettings><AutoInputGamutCompressionEnabled>true</AutoInputGamutCompressionEnabled><IsGraphicsWhiteSameAsProject>false</IsGraphicsWhiteSameAsProject><IsColorAwareEffectsEnabledSameAsProject>false</IsColorAwareEffectsEnabledSameAsProject><FrameRect>0,0,{width},{height}</FrameRect><OutputColorSpace>{{"baseColorProfile":{{"colorProfileName":"BT.709,8-bit,Display-Referred"}},"baseProfileType":1}}</OutputColorSpace><ComponentOwner Version="1"><Components ObjectRef="{video_chain}"/></ComponentOwner>"#,
            video_tracks.len() + 1
        ),
    );

    let (mix_chain, mix_panner) = audio_track_components(w, sample_tick);
    let sources: String = audio_tracks
        .iter()
        .enumerate()
        .map(|(i, t)| format!(r#"<Source Index="{i}" ObjectURef="{t}"/>"#))
        .collect();
    let inlet = w.obj(
        "AudioTrackInlet",
        class::AUDIO_TRACK_INLET,
        4,
        &format!(r#"<Sources Version="1">{sources}</Sources><AudioChannelLayout>{MONO}</AudioChannelLayout><ChannelType>0</ChannelType>"#),
    );
    let mix = w.obj(
        "AudioMixTrack",
        class::AUDIO_MIX_TRACK,
        4,
        &format!(
            r#"<AudioTrack Version="12"><ComponentOwner Version="1"><Components ObjectRef="{mix_chain}"/></ComponentOwner><Panner ObjectRef="{mix_panner}"/><ID>{}</ID><SubType>3</SubType><Assign>0</Assign><NextPannerID>4294967279</NextPannerID><ChannelType>0</ChannelType></AudioTrack>{}<Inlet ObjectRef="{inlet}"/>"#,
            new_uid(),
            track_header(1, MEDIA_TYPE_AUDIO, 0, false)
        ),
    );
    let atracks: String = audio_tracks
        .iter()
        .enumerate()
        .map(|(i, t)| format!(r#"<Track Index="{i}" ObjectURef="{t}"/>"#))
        .collect();
    let audio = w.obj(
        "AudioTrackGroup",
        class::AUDIO_TRACK_GROUP,
        6,
        &format!(
            r#"<TrackGroup Version="1"><Tracks Version="1">{atracks}</Tracks><NextTrackID>{}</NextTrackID><FrameRate>{sample_tick}</FrameRate></TrackGroup><MasterTrack ObjectRef="{mix}"/><ID>{}</ID><AutomationSafeFlags>0</AutomationSafeFlags><NumAdaptiveChannels>1</NumAdaptiveChannels>"#,
            audio_tracks.len() + 5,
            new_uid()
        ),
    );
    let data = w.obj(
        "DataTrackGroup",
        class::DATA_TRACK_GROUP,
        1,
        &format!(r#"<TrackGroup Version="1"><NextTrackID>1</NextTrackID><FrameRate>{frame}</FrameRate></TrackGroup>"#),
    );
    TrackGroups { video, audio, data }
}

fn sequence_track_groups_xml(g: &TrackGroups) -> String {
    format!(
        r#"<TrackGroups Version="1"><TrackGroup Version="1" Index="0"><First>{MEDIA_TYPE_VIDEO}</First><Second ObjectRef="{}"/></TrackGroup><TrackGroup Version="1" Index="1"><First>{MEDIA_TYPE_AUDIO}</First><Second ObjectRef="{}"/></TrackGroup><TrackGroup Version="1" Index="2"><First>{MEDIA_TYPE_DATA}</First><Second ObjectRef="{}"/></TrackGroup></TrackGroups>"#,
        g.video, g.audio, g.data
    )
}

fn sub_clip(w: &mut Writer, clip: u64, master: &str, name: &str, channel_group: u32) -> u64 {
    w.obj(
        "SubClip",
        class::SUB_CLIP,
        6,
        &format!(
            r#"<Clip ObjectRef="{clip}"/><MasterClip ObjectURef="{master}"/><Name>{}</Name><OrigChGrp>{channel_group}</OrigChGrp>"#,
            escape(name)
        ),
    )
}

/// Creates a merged clip of `video` and every channel of `audio`, synced by timecode.
/// Camera audio is left out, matching Premiere's "Remove audio from AV clips" option.
/// With `trim`, the clip only covers the frames where both video and audio exist.
pub fn merged_clip(w: &mut Writer, name: &str, video: &VideoSpec, audio: &AudioSpec, trim: bool) -> String {
    let t = trim
        .then(|| trimmed_merge_timing(video.start, video.duration, video.frame, audio.start, audio.duration))
        .flatten()
        .unwrap_or_else(|| merge_timing(video.start, video.duration, video.frame, audio.start, audio.duration));
    let n = audio.channels;
    let stem = std::path::Path::new(&audio.title)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| audio.title.clone());

    // One component master clip and one sequence track per production audio channel.
    let mut audio_items = Vec::new();
    for k in 0..n {
        let chains: Vec<Chain> = (0..n).map(|ch| audio_channel(w, audio, ch, video.time_display)).collect();
        let clips: Vec<u64> = chains
            .iter()
            .map(|c| audio_clip(w, c.source, Some(c.markers), Some((t.audio_in, t.audio_out)), true, true))
            .collect();
        let log = logging(
            w,
            Logging {
                clip_name: name,
                capture_mode: true,
                media_in: audio.start,
                media_out: audio.start + audio.duration,
                rate: audio.sample_tick,
                format: 200,
            },
        );
        let groups = channel_groups(w, Groups::Only(n, k));
        let props = [
            ("MZ.MergeClipUtils.AudioTrackNumberFromOriginalMergedClip", k.to_string()),
            (
                "MZ.MergeClipUtils.ComponentMasterClipOriginalName",
                format!("{stem} ({})", audio.channel_name(k as usize)),
            ),
        ];
        let component = master_clip(
            w,
            Master {
                properties: &props,
                logging: log,
                audio_chains: n as usize,
                clips: &clips,
                groups,
                name,
            },
        );
        let chain = chains[k as usize];
        let clip = audio_clip(
            w,
            chain.source,
            Some(chain.markers),
            Some((t.audio_item_in, t.audio_item_in + t.audio_end - t.audio_start)),
            false,
            true,
        );
        let sub = sub_clip(w, clip, &component, name, k);
        let components = default_audio_chain(w);
        let start = if t.audio_start != 0 {
            format!("<Start>{}</Start>", t.audio_start)
        } else {
            String::new()
        };
        let item = w.obj(
            "AudioClipTrackItem",
            class::AUDIO_CLIP_TRACK_ITEM,
            11,
            &format!(
                r#"<ClipTrackItem Version="8"><ComponentOwner Version="1"><Components ObjectRef="{components}"/></ComponentOwner><TrackItem Version="4">{start}<End>{}</End></TrackItem><SubClip ObjectRef="{sub}"/></ClipTrackItem><ID>{}</ID><PreRenderComponentChainHashVersion>1</PreRenderComponentChainHashVersion>"#,
                t.audio_end,
                new_uid()
            ),
        );
        audio_items.push(item);
    }

    // Video component master clip and its track item.
    let silent = VideoSpec {
        audio_channels: 0,
        ..video.clone()
    };
    let (vchain, _) = video_media(w, &silent);
    let vclip = video_clip(w, vchain.source, Some(vchain.markers), Some((t.video_in, t.video_out)), true, true);
    let vlog = logging(
        w,
        Logging {
            clip_name: name,
            capture_mode: false,
            media_in: video.start,
            media_out: video.start + video.duration,
            rate: video.frame,
            format: video.time_display,
        },
    );
    let vgroups = channel_groups(w, Groups::Empty);
    let video_stem = std::path::Path::new(&video.title)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| video.title.clone());
    let vprops = [("MZ.MergeClipUtils.ComponentMasterClipOriginalName", video_stem)];
    let vcomponent = master_clip(
        w,
        Master {
            properties: &vprops,
            logging: vlog,
            audio_chains: 0,
            clips: &[vclip],
            groups: vgroups,
            name,
        },
    );
    let vsub_clip = video_clip(w, vchain.source, Some(vchain.markers), Some((t.video_in, t.video_out)), false, true);
    let vsub = sub_clip(w, vsub_clip, &vcomponent, name, 0);
    let vcomponents = w.obj(
        "VideoComponentChain",
        class::VIDEO_COMPONENT_CHAIN,
        3,
        r#"<DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><DefaultMotionComponentID>1</DefaultMotionComponentID><DefaultOpacityComponentID>2</DefaultOpacityComponentID><ComponentChain Version="3"></ComponentChain>"#,
    );
    let vstart = if t.video_start != 0 {
        format!("<Start>{}</Start>", t.video_start)
    } else {
        String::new()
    };
    let video_item = w.obj(
        "VideoClipTrackItem",
        class::VIDEO_CLIP_TRACK_ITEM,
        8,
        &format!(
            r#"<ClipTrackItem Version="8"><ComponentOwner Version="1"><Components ObjectRef="{vcomponents}"/></ComponentOwner><TrackItem Version="4">{vstart}<End>{}</End></TrackItem><SubClip ObjectRef="{vsub}"/></ClipTrackItem><ToneMapSettings>{{"peak":-1,"version":3}}</ToneMapSettings><FrameRect>0,0,{},{}</FrameRect><PixelAspectRatio>1,1</PixelAspectRatio>"#,
            t.video_end, video.width, video.height
        ),
    );

    // Tracks, link and the hidden merged-clip sequence.
    let video_track = video_clip_track(w, 1, 0, &[video_item]);
    let audio_tracks: Vec<String> = audio_items
        .iter()
        .enumerate()
        .map(|(i, item)| audio_clip_track(w, 4 + i as u32, i, &[*item], audio.sample_tick))
        .collect();
    let groups = track_groups(w, &[video_track], &audio_tracks, video.frame, audio.sample_tick, video.width, video.height);
    let link_items: String = audio_items
        .iter()
        .chain(std::iter::once(&video_item))
        .enumerate()
        .map(|(i, t)| format!(r#"<TrackItem Index="{i}" ObjectRef="{t}"/>"#))
        .collect();
    let link = w.obj(
        "Link",
        class::LINK,
        1,
        &format!(r#"<TrackItemGroup Version="1"><TrackItems Version="1">{link_items}</TrackItems></TrackItemGroup>"#),
    );
    let sequence = w.uobj(
        "Sequence",
        class::SEQUENCE,
        12,
        &format!(
            r#"<Node Version="1"><Properties Version="1"><MZ.EditLine>0</MZ.EditLine><MZ.Sequence.AudioTimeDisplayFormat>200</MZ.Sequence.AudioTimeDisplayFormat><MZ.Sequence.VideoTimeDisplayFormat>{}</MZ.Sequence.VideoTimeDisplayFormat><MZ.WorkInPoint>0</MZ.WorkInPoint><MZ.WorkOutPoint>{}</MZ.WorkOutPoint><MZ.MergedClip.AudioTimecodeBaseTrackNumber>-1</MZ.MergedClip.AudioTimecodeBaseTrackNumber><MZ.Sequence.PreviewFrameSizeWidth>{}</MZ.Sequence.PreviewFrameSizeWidth><MZ.Sequence.PreviewFrameSizeHeight>{}</MZ.Sequence.PreviewFrameSizeHeight><BE.Sequence.IsMergedClip>true</BE.Sequence.IsMergedClip><MZ.ZeroPoint>{}</MZ.ZeroPoint></Properties></Node><PersistentGroupContainer Version="1"><LinkContainer Version="1"><Links Version="1"><Link Index="0" ObjectRef="{link}"/></Links></LinkContainer></PersistentGroupContainer>{}<Name>{}</Name>"#,
            video.time_display,
            t.duration,
            video.width,
            video.height,
            t.zero_point,
            sequence_track_groups_xml(&groups),
            escape(name)
        ),
    );

    // The merged master clip the user sees in the bin.
    let aseq = w.obj(
        "AudioSequenceSource",
        class::AUDIO_SEQUENCE_SOURCE,
        7,
        &format!(r#"<SequenceSource Version="4"><Content Version="10"></Content><Sequence ObjectURef="{sequence}"/></SequenceSource><OriginalDuration>{}</OriginalDuration>"#, t.duration),
    );
    let vseq = w.obj(
        "VideoSequenceSource",
        class::VIDEO_SEQUENCE_SOURCE,
        3,
        &format!(r#"<SequenceSource Version="4"><Content Version="10"></Content><Sequence ObjectURef="{sequence}"/></SequenceSource><OriginalDuration>{}</OriginalDuration>"#, t.duration),
    );
    let master_audio = audio_clip(w, aseq, None, None, true, false);
    let master_video = video_clip(w, vseq, None, None, true, false);
    let log = logging(
        w,
        Logging {
            clip_name: &video.title,
            capture_mode: false,
            media_in: video.start,
            media_out: video.start + video.duration,
            rate: video.frame,
            format: video.time_display,
        },
    );
    let groups = channel_groups(w, Groups::All(n));
    let props = [
        ("monitor.take.audio", "true".to_string()),
        ("monitor.take.video", "true".to_string()),
    ];
    let master = master_clip(
        w,
        Master {
            properties: &props,
            logging: log,
            audio_chains: n as usize,
            clips: &[master_audio, master_video],
            groups,
            name,
        },
    );
    clip_project_item(w, name, &master)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: i64 = 10_594_584_000;

    #[test]
    fn timing_matches_premiere_when_audio_starts_first() {
        // 2-001.WAV against A001C001 from a Premiere-made merged clip.
        let t = merge_timing(2_703_324_648_024_000, 15_743_551_824_000, FRAME, 2_703_261_339_828_000, 16_045_227_576_000);
        assert_eq!(t.zero_point, 2_703_261_080_520_000);
        assert_eq!(t.audio_in, -259_308_000);
        assert_eq!(t.audio_out, 16_039_940_868_000);
        assert_eq!(t.audio_start, 0);
        assert_eq!(t.audio_end, 16_029_605_592_000);
        assert_eq!(t.video_start, 63_567_504_000);
        assert_eq!(t.video_end, 15_807_119_328_000);
        assert_eq!(t.duration, 16_029_605_592_000);
    }

    #[test]
    fn trimmed_timing_keeps_only_the_overlap() {
        // Audio rolls before and after the video: the clip is exactly the video.
        let (v, vd) = (2_703_324_648_024_000, 15_743_551_824_000);
        let (a, ad) = (2_703_261_339_828_000, 16_045_227_576_000);
        let t = trimmed_merge_timing(v, vd, FRAME, a, ad).unwrap();
        assert_eq!(t.zero_point, v);
        assert_eq!((t.video_in, t.video_out), (0, vd));
        assert_eq!((t.video_start, t.video_end), (0, vd));
        assert_eq!(t.audio_in, v - a);
        assert_eq!(t.audio_item_in, v - a);
        assert_eq!(t.audio_out - t.audio_in, vd);
        assert_eq!(t.duration, vd);

        // Video rolls first and audio stops early: start and end snap inward to whole frames.
        let a = v + 100 * FRAME + 1_000 * 5_292_000;
        let ad = 200 * FRAME;
        let t = trimmed_merge_timing(v, vd, FRAME, a, ad).unwrap();
        assert_eq!(t.zero_point, v + 101 * FRAME);
        assert_eq!(t.video_in, 101 * FRAME);
        assert_eq!(t.duration, 199 * FRAME);
        assert_eq!(t.video_out - t.video_in, t.duration);
        assert!(t.audio_in >= 0 && t.audio_out <= ad);

        assert!(trimmed_merge_timing(v, vd, FRAME, v + vd, ad).is_none());
    }

    #[test]
    fn merged_clip_references_resolve() {
        let mut w = Writer::new(5000);
        let video = VideoSpec {
            path: "/media/A001C001.mov".into(),
            title: "A001C001.mov".into(),
            start: 255_161 * FRAME,
            duration: 1486 * FRAME,
            frame: FRAME,
            time_display: 110,
            width: 3200,
            height: 1800,
            audio_channels: 0,
            sample_tick: 5_292_000,
        };
        let audio = AudioSpec {
            path: "/media/2-001 & co.WAV".into(),
            title: "2-001 & co.WAV".into(),
            start: 510_820_359 * 5_292_000,
            duration: 3_031_978 * 5_292_000,
            sample_tick: 5_292_000,
            channels: 2,
            channel_names: vec!["MixL".into(), "Boom".into()],
        };
        merged_clip(&mut w, "2-001", &video, &audio, false);
        let all = w.objects().join("\n");
        assert!(all.contains("&amp; co.WAV"));
        let ids: std::collections::HashSet<String> = regex::Regex::new(r#"Object(?:U)?ID="([^"]+)""#)
            .unwrap()
            .captures_iter(&all)
            .map(|c| c[1].to_string())
            .collect();
        for cap in regex::Regex::new(r#"Object(?:U)?Ref="([^"]+)""#).unwrap().captures_iter(&all) {
            assert!(ids.contains(&cap[1]), "dangling reference {}", &cap[1]);
        }
        assert_eq!(all.matches("<MasterClip ObjectUID").count(), 4);
        assert_eq!(all.matches("<AudioClipTrack ObjectUID").count(), 2);
    }
}
