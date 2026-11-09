//! Decides names and bins for synced media, then writes them into a Premiere project.

use std::collections::HashMap;

use crate::media::{MediaFile, MediaKind, Slate};
use crate::naming;
use crate::prproj::objects::{self, AudioSpec, VideoSpec};
use crate::prproj::Project;
use crate::sync::SyncPlan;
use crate::time::{self, Rate};

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
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct BuildReport {
    pub merged_clips: usize,
    pub multicam_sequences: usize,
    pub plain_clips: usize,
    pub bins_used: usize,
    pub warnings: Vec<String>,
}

pub(crate) const DEFAULT_RATE: Rate = Rate::new(24000, 1001);

fn video_spec(f: &MediaFile, with_audio: bool) -> VideoSpec {
    let rate = f.rate.unwrap_or(DEFAULT_RATE);
    VideoSpec {
        path: f.path.to_string_lossy().into_owned(),
        title: f.file_name(),
        start: f.start.unwrap_or(0),
        duration: f.duration,
        frame: rate.frame_ticks(),
        time_display: rate.premiere_time_display(f.drop_frame),
        width: f.width.max(1),
        height: f.height.max(1),
        audio_channels: if with_audio { f.audio_channels } else { 0 },
        sample_tick: time::sample_ticks(f.sample_rate),
    }
}

fn audio_spec(f: &MediaFile) -> AudioSpec {
    AudioSpec {
        path: f.path.to_string_lossy().into_owned(),
        title: f.file_name(),
        start: f.start.unwrap_or(0),
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
        if v.start.is_none() || a.start.is_none() {
            warnings.push(format!("{} has no timecode and was skipped", v.file_name()));
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

    if opts.multicam && !plan.multicam.is_empty() {
        warnings.push(format!(
            "{} multicam group(s) detected. Angles were added as merged clips; multicam source sequences are not generated yet.",
            plan.multicam.len()
        ));
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
                &video_spec(&files[video], false),
                &audio_spec(&files[audio]),
                opts.trim_to_overlap,
            ),
            ItemKind::Plain(i) => {
                let f = &files[i];
                match f.kind {
                    MediaKind::Video => objects::plain_video_clip(project.writer(), &item.name, &video_spec(f, true)),
                    MediaKind::Audio => {
                        objects::plain_audio_clip(project.writer(), &item.name, &audio_spec(f), DEFAULT_RATE.premiere_time_display(false))
                    }
                }
            }
        };
        project.add_to_bin(&parent, &uid);
    }
    report_for(&items, warnings)
}

/// Writes a Final Cut Pro 7 XML timeline of every synced clip, for Premiere or Resolve to import.
/// Bins, multicam and unsynced files don't apply to a single timeline and are ignored.
pub fn build_xml(files: &[MediaFile], plan: &SyncPlan, timeline_name: &str, opts: &BuildOptions) -> (String, BuildReport) {
    let opts = BuildOptions {
        scene_bins: false,
        shot_bins: false,
        multicam: false,
        import_unsynced: false,
        ..opts.clone()
    };
    let (items, warnings) = layout(files, plan, &opts);
    let xml = crate::xmeml::export(files, &items, timeline_name, opts.trim_to_overlap);
    (xml, report_for(&items, warnings))
}
