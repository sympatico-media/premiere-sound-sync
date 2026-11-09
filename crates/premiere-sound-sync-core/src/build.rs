//! Decides names and bins for synced media, then writes them into a Premiere project.

use std::collections::HashMap;

use crate::media::{MediaFile, MediaKind, Slate};
use crate::naming;
use crate::prproj::objects::{self, AudioSpec, VideoSpec};
use crate::prproj::Project;
use crate::sync::SyncPlan;
use crate::time::{self, Rate};
use crate::xmeml;

/// How an XML timeline lays out its synced takes.
///
/// A Premiere `.prproj` can only safely hold what Premiere itself writes for "Merge Clips":
/// there's no public account of the binary fields a hand-authored, visible Sequence or a
/// multicam clip would need (see the module docs on [`crate::xmeml`]), so `.prproj` output stays
/// merged-clips-only. XML has no such risk - it's a plain, documented interchange format - so
/// that's where multi-sequence and multicam-ready layouts live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlPrimary {
    /// Every synced take on one timeline, back to back in shoot order.
    All,
    /// One sequence per take, with every camera angle on its own video track, already aligned.
    /// Select one in Premiere and choose "Create Multi-Camera Source Sequence" to finish the
    /// multicam clip, or use Resolve / another NLE's own sync tooling on it directly.
    PerTake,
}

impl Default for XmlPrimary {
    fn default() -> Self {
        XmlPrimary::All
    }
}

#[derive(Debug, Clone)]
pub struct BuildOptions {
    pub rename_from_metadata: bool,
    pub name_template: String,
    pub scene_bins: bool,
    pub shot_bins: bool,
    pub multicam: bool,
    /// Trim merged clips to the range where both video and audio exist.
    pub trim_to_overlap: bool,
    pub import_unsynced: bool,
    /// How the XML timeline's main layout works. Ignored by `.prproj` output.
    pub xml_primary: XmlPrimary,
    /// Also write one extra XML sequence per scene, takes in slate order.
    pub xml_scene_stringouts: bool,
    /// Also write one extra XML sequence per shoot day.
    pub xml_day_sequences: bool,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            rename_from_metadata: true,
            name_template: naming::DEFAULT_TEMPLATE.to_string(),
            scene_bins: true,
            shot_bins: false,
            multicam: true,
            trim_to_overlap: true,
            import_unsynced: true,
            xml_primary: XmlPrimary::default(),
            xml_scene_stringouts: false,
            xml_day_sequences: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct BuildReport {
    pub merged_clips: usize,
    pub multicam_sequences: usize,
    pub plain_clips: usize,
    pub bins_used: usize,
    /// Number of synced takes written (XML output only; a take can be more than one camera
    /// angle, when angles were grouped together).
    pub takes: usize,
    /// Number of `<sequence>` elements written (XML output only).
    pub sequences: usize,
    pub warnings: Vec<String>,
}

pub(crate) const DEFAULT_RATE: Rate = Rate::new(24000, 1001);

fn video_spec(f: &MediaFile, with_audio: bool, position: i64) -> VideoSpec {
    let rate = f.rate.unwrap_or(DEFAULT_RATE);
    VideoSpec {
        path: f.path.to_string_lossy().into_owned(),
        title: f.file_name(),
        start: f.start.unwrap_or(0),
        position,
        duration: f.duration,
        frame: rate.frame_ticks(),
        time_display: rate.premiere_time_display(f.drop_frame),
        width: f.width.max(1),
        height: f.height.max(1),
        audio_channels: if with_audio { f.audio_channels } else { 0 },
        sample_tick: time::sample_ticks(f.sample_rate),
    }
}

fn audio_spec(f: &MediaFile, position: i64) -> AudioSpec {
    AudioSpec {
        path: f.path.to_string_lossy().into_owned(),
        title: f.file_name(),
        start: f.start.unwrap_or(0),
        position,
        duration: f.duration,
        sample_tick: time::sample_ticks(f.sample_rate),
        channels: f.audio_channels.max(1),
        channel_names: f.channel_names.clone(),
    }
}

/// What one output item is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// Video synced to production audio (indices into the file list).
    Merged { video: usize, audio: usize },
    /// A file with nothing to sync to.
    Plain(usize),
}

/// One clip to create, with its final name and bin path below the target bin.
#[derive(Debug, Clone)]
pub struct PlannedItem {
    pub name: String,
    pub bin: Vec<String>,
    pub kind: ItemKind,
}

/// Names and bins for every item, shared by every output format.
pub fn layout(files: &[MediaFile], plan: &SyncPlan, opts: &BuildOptions) -> (Vec<PlannedItem>, Vec<String>) {
    let mut items = Vec::new();
    let mut warnings = Vec::new();
    let name = |slate: &Slate, camera: Option<&str>, fallback: &str| {
        if opts.rename_from_metadata {
            naming::render(&opts.name_template, slate, camera, fallback)
        } else {
            fallback.to_string()
        }
    };
    let slate_bins = |slate: &Slate| {
        let mut path = Vec::new();
        if opts.scene_bins {
            path.extend(naming::scene_bin_name(slate));
        }
        if opts.shot_bins {
            path.extend(naming::shot_bin_name(slate));
        }
        path
    };

    for pair in &plan.merged {
        let (v, a) = (&files[pair.video], &files[pair.audio]);
        if plan.positions[pair.video].is_none() || plan.positions[pair.audio].is_none() {
            warnings.push(format!("{} has no sync position and was skipped", v.file_name()));
            continue;
        }
        let slate = a.slate.clone().or(&v.slate);
        let fallback = if opts.rename_from_metadata { a.stem() } else { v.stem() };
        let mut clip_name = name(&slate, v.camera.as_deref(), &fallback);
        let in_multicam = plan.multicam.iter().any(|g| g.angles.iter().flatten().any(|&i| i == pair.video));
        if in_multicam {
            if let Some(cam) = v.camera.as_deref() {
                clip_name = format!("{clip_name} {cam}");
            }
        }
        items.push(PlannedItem {
            name: clip_name,
            bin: slate_bins(&slate),
            kind: ItemKind::Merged { video: pair.video, audio: pair.audio },
        });
    }

    if opts.import_unsynced {
        for &i in &plan.unsynced {
            let f = &files[i];
            items.push(PlannedItem {
                name: name(&f.slate, f.camera.as_deref(), &f.stem()),
                bin: vec!["Unsynced".to_string()],
                kind: ItemKind::Plain(i),
            });
        }
    }
    (items, warnings)
}

fn report_for(items: &[PlannedItem], warnings: Vec<String>) -> BuildReport {
    let mut bins = std::collections::HashSet::new();
    for item in items {
        for depth in 1..=item.bin.len() {
            bins.insert(item.bin[..depth].to_vec());
        }
    }
    BuildReport {
        merged_clips: items.iter().filter(|i| matches!(i.kind, ItemKind::Merged { .. })).count(),
        multicam_sequences: 0,
        plain_clips: items.iter().filter(|i| matches!(i.kind, ItemKind::Plain(_))).count(),
        bins_used: bins.len(),
        takes: 0,
        sequences: 0,
        warnings,
    }
}

/// Adds merged clips and unsynced clips to `project` under `target_bin`.
pub fn build(files: &[MediaFile], plan: &SyncPlan, project: &mut Project, target_bin: &str, opts: &BuildOptions) -> BuildReport {
    let (items, warnings) = layout(files, plan, opts);
    let mut bins: HashMap<Vec<String>, String> = HashMap::new();
    for item in &items {
        let mut parent = target_bin.to_string();
        for depth in 1..=item.bin.len() {
            let path = item.bin[..depth].to_vec();
            parent = match bins.get(&path) {
                Some(uid) => uid.clone(),
                None => {
                    let uid = project.ensure_bin(&parent, &item.bin[depth - 1]);
                    bins.insert(path, uid.clone());
                    uid
                }
            };
        }
        let uid = match item.kind {
            ItemKind::Merged { video, audio } => objects::merged_clip(
                project.writer(),
                &item.name,
                &video_spec(&files[video], false, plan.positions[video].unwrap_or(0)),
                &audio_spec(&files[audio], plan.positions[audio].unwrap_or(0)),
                opts.trim_to_overlap,
            ),
            ItemKind::Plain(i) => {
                let f = &files[i];
                let position = plan.positions[i].unwrap_or_else(|| f.start.unwrap_or(0));
                match f.kind {
                    MediaKind::Video => objects::plain_video_clip(project.writer(), &item.name, &video_spec(f, true, position)),
                    MediaKind::Audio => {
                        objects::plain_audio_clip(project.writer(), &item.name, &audio_spec(f, position), DEFAULT_RATE.premiere_time_display(false))
                    }
                }
            }
        };
        project.add_to_bin(&parent, &uid);
    }
    report_for(&items, warnings)
}

/// One take: the camera angle(s) that share a production audio recording. Angles come from
/// `plan.multicam` when the cameras line up with each other; everything else is a single angle.
#[derive(Debug, Clone)]
struct TakeGroup {
    name: String,
    scene: Option<String>,
    /// Day the take was recorded on (see [`crate::time::ymd_utc`]), when known.
    day: Option<String>,
    videos: Vec<usize>,
    audio: usize,
}

/// Groups every synced video into its take, using `plan.multicam` to gather camera angles that
/// belong together. Skips anything missing a sync position. Order is shoot order (by position).
fn group_takes(files: &[MediaFile], plan: &SyncPlan, opts: &BuildOptions) -> (Vec<TakeGroup>, Vec<String>) {
    let render_name = |slate: &Slate, camera: Option<&str>, fallback: &str| {
        if opts.rename_from_metadata {
            naming::render(&opts.name_template, slate, camera, fallback)
        } else {
            fallback.to_string()
        }
    };
    let mut group_of: HashMap<usize, usize> = HashMap::new();
    for (gi, g) in plan.multicam.iter().enumerate() {
        for &v in g.angles.iter().flatten() {
            group_of.insert(v, gi);
        }
    }
    let mut by_group: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
    let mut solo: Vec<usize> = Vec::new();
    for pair in &plan.merged {
        match group_of.get(&pair.video) {
            Some(&gi) => by_group.entry(gi).or_default().push(pair.video),
            None => solo.push(pair.video),
        }
    }

    let mut warnings = Vec::new();
    let mut videos_and_audio: Vec<Vec<usize>> = by_group.into_values().chain(solo.into_iter().map(|v| vec![v])).collect();
    for group in &mut videos_and_audio {
        group.sort_by_key(|&v| plan.positions[v]);
    }

    let mut takes = Vec::new();
    for videos in videos_and_audio {
        let Some(audio) = videos.iter().find_map(|&v| plan.merged.iter().find(|m| m.video == v).map(|m| m.audio)) else { continue };
        let main = videos[0];
        if plan.positions[main].is_none() || plan.positions[audio].is_none() {
            warnings.push(format!("{} has no sync position and was skipped", files[main].file_name()));
            continue;
        }
        let slate = files[audio].slate.clone().or(&files[main].slate);
        let fallback = if opts.rename_from_metadata { files[audio].stem() } else { files[main].stem() };
        let name = render_name(&slate, files[main].camera.as_deref(), &fallback);
        let day = files[main].recorded().or_else(|| files[audio].recorded()).and_then(time::ymd_utc);
        takes.push(TakeGroup { name, scene: slate.scene, day, videos, audio });
    }
    takes.sort_by_key(|t| plan.positions[t.videos[0]]);
    (takes, warnings)
}

fn report_for_xml(sequences: usize, takes: usize, warnings: Vec<String>) -> BuildReport {
    BuildReport {
        takes,
        sequences,
        warnings,
        ..Default::default()
    }
}

/// Writes a Final Cut Pro 7 XML project of one or more synced-take sequences, for Premiere Pro
/// or DaVinci Resolve to import. Bins and unsynced files don't apply to XML and are left out.
pub fn build_xml(files: &[MediaFile], plan: &SyncPlan, doc_name: &str, opts: &BuildOptions) -> (String, BuildReport) {
    let (groups, warnings) = group_takes(files, plan, opts);
    let takes: Vec<xmeml::Take> = groups
        .iter()
        .map(|g| xmeml::Take {
            name: g.name.clone(),
            videos: g.videos.iter().map(|&v| &files[v]).collect(),
            video_positions: g.videos.iter().map(|&v| plan.positions[v].unwrap_or(0)).collect(),
            audio: &files[g.audio],
            audio_position: plan.positions[g.audio].unwrap_or(0),
        })
        .collect();

    let mut sequences: Vec<(String, Vec<usize>)> = Vec::new();
    match opts.xml_primary {
        XmlPrimary::All => sequences.push((doc_name.to_string(), (0..groups.len()).collect())),
        XmlPrimary::PerTake => {
            for (i, g) in groups.iter().enumerate() {
                sequences.push((g.name.clone(), vec![i]));
            }
        }
    }
    if opts.xml_scene_stringouts {
        let mut by_scene: std::collections::BTreeMap<Option<String>, Vec<usize>> = std::collections::BTreeMap::new();
        for (i, g) in groups.iter().enumerate() {
            by_scene.entry(g.scene.clone()).or_default().push(i);
        }
        for (scene, idxs) in by_scene {
            let name = match scene {
                Some(s) => format!("Scene {s} Stringout"),
                None => "No Scene Stringout".to_string(),
            };
            sequences.push((name, idxs));
        }
    }
    if opts.xml_day_sequences {
        let mut by_day: std::collections::BTreeMap<Option<String>, Vec<usize>> = std::collections::BTreeMap::new();
        for (i, g) in groups.iter().enumerate() {
            by_day.entry(g.day.clone()).or_default().push(i);
        }
        for (day, idxs) in by_day {
            let name = day.unwrap_or_else(|| "Unknown Day".to_string());
            sequences.push((name, idxs));
        }
    }

    let xml = xmeml::export(&takes, &sequences, opts.trim_to_overlap);
    (xml, report_for_xml(sequences.len(), takes.len(), warnings))
}
