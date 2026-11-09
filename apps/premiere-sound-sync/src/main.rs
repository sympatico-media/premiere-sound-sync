#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use premiere_sound_sync_core::time::{Rate, TICKS_PER_SECOND};
use premiere_sound_sync_core::{
    build, build_xml, naming, plan, probe, scan, waveform, BinInfo, BuildOptions, MediaFile, MediaKind, Project, SyncMethod, SyncOptions, SyncPlan,
    Timeline,
};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel, Weak};
use studio_ui::{dialogs, FileDrop};

mod update;

slint::include_modules!();

#[derive(Default)]
struct State {
    sources: Vec<PathBuf>,
    files: Vec<MediaFile>,
    failures: Vec<(PathBuf, String)>,
    plan: SyncPlan,
    generation: u64,
    existing: Option<(PathBuf, Vec<BinInfo>)>,
    output: Option<PathBuf>,
    last_output: Option<PathBuf>,
    icons: HashMap<PathBuf, slint::Image>,
    fingerprints: HashMap<PathBuf, waveform::Fingerprint>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

fn main() -> anyhow::Result<()> {
    studio_ui::set_app_name("Premiere Sync");
    studio_ui::init_backend()?;
    let ui = AppWindow::new()?;
    // AppKit resets the icon while it finishes launching, so set it once the event loop runs.
    slint::Timer::single_shot(Duration::from_millis(100), || {
        studio_ui::set_app_icon(include_bytes!("../assets/AppIcon.png"));
    });
    ui.set_name_template(naming::DEFAULT_TEMPLATE.into());
    if let Some(icon) = studio_ui::system_icons::for_file_type("public.data", 64) {
        ui.set_new_project_icon(icon);
    }
    if let Some(icon) = studio_ui::system_icons::for_file_type("prproj", 64) {
        ui.set_existing_project_icon(icon);
    }
    if let Some(icon) = studio_ui::system_icons::for_file_type("xml", 64) {
        ui.set_xml_icon(icon);
    }
    set_status(&ui, "Add a project folder to begin.", 0);

    let rescan_timer = Rc::new(slint::Timer::default());

    {
        let weak = ui.as_weak();
        let timer = rescan_timer.clone();
        studio_ui::on_file_drop(&ui.window(), move |event| {
            let Some(ui) = weak.upgrade() else { return };
            match event {
                FileDrop::Hovering => ui.set_drop_active(true),
                FileDrop::Cancelled => ui.set_drop_active(false),
                FileDrop::Dropped(path) => {
                    ui.set_drop_active(false);
                    add_sources(vec![path]);
                    let weak = ui.as_weak();
                    timer.start(slint::TimerMode::SingleShot, Duration::from_millis(150), move || {
                        if let Some(ui) = weak.upgrade() {
                            sources_changed(&ui);
                        }
                    });
                }
            }
        });
    }

    ui.on_add_sources({
        let weak = ui.as_weak();
        move || {
            let ui = weak.unwrap();
            let picked = dialogs::pick_folders("Add folders");
            if picked.is_empty() {
                return;
            }
            add_sources(picked);
            sources_changed(&ui);
        }
    });

    ui.on_remove_source({
        let weak = ui.as_weak();
        move |index| {
            let ui = weak.unwrap();
            with_state(|s| {
                if (index as usize) < s.sources.len() {
                    s.sources.remove(index as usize);
                }
            });
            sources_changed(&ui);
        }
    });

    ui.on_clear_sources({
        let weak = ui.as_weak();
        move || {
            let ui = weak.unwrap();
            with_state(|s| s.sources.clear());
            sources_changed(&ui);
        }
    });

    ui.on_options_changed({
        let weak = ui.as_weak();
        move || {
            let ui = weak.unwrap();
            start_analysis(&ui);
        }
    });

    ui.on_choose_new({
        let weak = ui.as_weak();
        move || {
            let ui = weak.unwrap();
            if ui.get_dest_mode() != 0 {
                ui.set_dest_mode(0);
                with_state(|s| s.output = None);
                refresh_destination(&ui);
            }
        }
    });

    ui.on_choose_xml({
        let weak = ui.as_weak();
        move || {
            let ui = weak.unwrap();
            if ui.get_dest_mode() != 2 {
                ui.set_dest_mode(2);
                with_state(|s| s.output = None);
                refresh_destination(&ui);
            }
        }
    });

    ui.on_choose_existing({
        let weak = ui.as_weak();
        move || {
            let ui = weak.unwrap();
            let Some(path) = dialogs::pick_file("Choose a Premiere Pro project", Some(("Premiere Pro Project", &["prproj"])))
            else {
                if with_state(|s| s.existing.is_some()) {
                    ui.set_dest_mode(1);
                    refresh_destination(&ui);
                }
                return;
            };
            open_existing(&ui, path);
        }
    });

    ui.on_change_output({
        let weak = ui.as_weak();
        move || {
            let ui = weak.unwrap();
            if let Some(path) = ask_output(&ui) {
                with_state(|s| s.output = Some(path));
                refresh_destination(&ui);
            }
        }
    });

    ui.on_build({
        let weak = ui.as_weak();
        move || {
            let ui = weak.unwrap();
            start_build(&ui);
        }
    });

    ui.on_reveal(|| {
        if let Some(path) = with_state(|s| s.last_output.clone()) {
            studio_ui::reveal_in_file_manager(&path);
        }
    });

    ui.on_reveal_media(|index| {
        let path = with_state(|s| {
            let index = index as usize;
            if index < s.files.len() {
                Some(s.files[index].path.clone())
            } else {
                s.failures.get(index - s.files.len()).map(|(path, _)| path.clone())
            }
        });
        if let Some(path) = path {
            studio_ui::reveal_in_file_manager(&path);
        }
    });

    ui.on_open_update({
        let weak = ui.as_weak();
        move || studio_ui::open_url(&weak.unwrap().get_update_url())
    });

    {
        let weak = ui.as_weak();
        std::thread::spawn(move || {
            if let Some(release) = update::check() {
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.set_update_version(release.version.into());
                    ui.set_update_url(release.url.into());
                });
            }
        });
    }

    refresh_media(&ui);
    let initial: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).filter(|p| p.exists()).collect();
    if initial.is_empty() {
        refresh_sources(&ui);
        refresh_destination(&ui);
    } else {
        add_sources(initial);
        sources_changed(&ui);
    }
    ui.run()?;
    Ok(())
}

fn set_status(ui: &AppWindow, text: &str, tone: i32) {
    ui.set_status(text.into());
    ui.set_status_tone(tone);
}

fn add_sources(paths: Vec<PathBuf>) {
    with_state(|s| {
        for p in paths {
            if !s.sources.contains(&p) {
                s.sources.push(p);
            }
        }
    });
}

fn sources_changed(ui: &AppWindow) {
    ui.set_can_reveal(false);
    refresh_sources(ui);
    start_scan(ui);
    refresh_destination(ui);
}

fn refresh_sources(ui: &AppWindow) {
    let items: Vec<SourceItem> = with_state(|s| {
        s.sources
            .iter()
            .map(|p| {
                let is_folder = p.is_dir();
                let found = s.files.iter().filter(|f| f.path.starts_with(p)).count();
                let subtitle = if is_folder {
                    match found {
                        0 => String::new(),
                        1 => "1 file".to_string(),
                        n => format!("{n} files"),
                    }
                } else {
                    p.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                };
                let icon = s
                    .icons
                    .entry(p.clone())
                    .or_insert_with(|| studio_ui::system_icons::for_path(p, 64).unwrap_or_default())
                    .clone();
                SourceItem {
                    title: display_name(p).into(),
                    subtitle: subtitle.into(),
                    is_folder,
                    icon,
                }
            })
            .collect()
    });
    ui.set_sources(ModelRc::new(VecModel::from(items)));
}

fn display_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

fn start_scan(ui: &AppWindow) {
    let (sources, generation) = with_state(|s| {
        s.generation += 1;
        (s.sources.clone(), s.generation)
    });
    if sources.is_empty() {
        with_state(|s| {
            s.files.clear();
            s.failures.clear();
            s.plan = SyncPlan::default();
        });
        ui.set_scanning(false);
        refresh_media(ui);
        refresh_sources(ui);
        set_status(ui, "Add a project folder to begin.", 0);
        return;
    }
    ui.set_scanning(true);
    ui.set_scan_progress(0.0);
    set_status(ui, "Looking for media…", 0);
    let weak = ui.as_weak();
    std::thread::spawn(move || {
        let paths = scan::collect(&sources);
        let total = paths.len();
        let done = Arc::new(AtomicUsize::new(0));
        let results: Arc<Mutex<Vec<(usize, Result<MediaFile, String>)>>> = Arc::new(Mutex::new(Vec::new()));
        let next = Arc::new(AtomicUsize::new(0));
        let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
        std::thread::scope(|scope| {
            for _ in 0..workers {
                let (paths, done, results, next, weak) = (&paths, done.clone(), results.clone(), next.clone(), weak.clone());
                scope.spawn(move || loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    if i >= paths.len() {
                        break;
                    }
                    let r = probe::probe(&paths[i]).map_err(|e| e.to_string());
                    results.lock().unwrap().push((i, r));
                    let n = done.fetch_add(1, Ordering::SeqCst) + 1;
                    let progress = n as f32 / total.max(1) as f32;
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        if with_state(|s| s.generation) == generation {
                            ui.set_scan_progress(progress);
                            ui.set_status(format!("Reading media… {n} of {total}").into());
                        }
                    });
                });
            }
        });
        let mut results = Arc::try_unwrap(results).map(|m| m.into_inner().unwrap()).unwrap_or_default();
        results.sort_by_key(|r| r.0);
        let _ = weak.upgrade_in_event_loop(move |ui| finish_scan(&ui, generation, paths, results));
    });
}

fn finish_scan(ui: &AppWindow, generation: u64, paths: Vec<PathBuf>, results: Vec<(usize, Result<MediaFile, String>)>) {
    if with_state(|s| s.generation) != generation {
        return;
    }
    with_state(|s| {
        s.files.clear();
        s.failures.clear();
        for (i, r) in results {
            match r {
                Ok(f) => s.files.push(f),
                Err(e) => s.failures.push((paths[i].clone(), e)),
            }
        }
    });
    ui.set_scanning(false);
    refresh_sources(ui);
    start_analysis(ui);
}

fn sync_options(ui: &AppWindow) -> SyncOptions {
    SyncOptions {
        timecode: ui.get_sync_timecode(),
        waveform: ui.get_sync_waveform(),
        multicam: ui.get_multicam(),
    }
}

/// Replans the sync, first fingerprinting any file waveform sync needs that isn't cached yet.
/// Safe to call any time the files, the sync method checkboxes, or `multicam` change.
fn start_analysis(ui: &AppWindow) {
    let opts = sync_options(ui);
    let generation = with_state(|s| s.generation);
    if !opts.waveform {
        with_state(|s| s.plan = plan(&s.files, &s.fingerprints, &opts));
        ui.set_analyzing(false);
        finish_planning(ui);
        return;
    }
    let missing: Vec<PathBuf> = with_state(|s| s.files.iter().map(|f| f.path.clone()).filter(|p| !s.fingerprints.contains_key(p)).collect());
    if missing.is_empty() {
        with_state(|s| s.plan = plan(&s.files, &s.fingerprints, &opts));
        ui.set_analyzing(false);
        finish_planning(ui);
        return;
    }
    ui.set_analyzing(true);
    ui.set_analyze_progress(0.0);
    set_status(ui, "Analyzing audio…", 0);
    let weak = ui.as_weak();
    std::thread::spawn(move || {
        let total = missing.len();
        let done = Arc::new(AtomicUsize::new(0));
        let results: Arc<Mutex<Vec<(PathBuf, Result<waveform::Fingerprint, String>)>>> = Arc::new(Mutex::new(Vec::new()));
        let next = Arc::new(AtomicUsize::new(0));
        let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
        std::thread::scope(|scope| {
            for _ in 0..workers {
                let (missing, done, results, next, weak) = (&missing, done.clone(), results.clone(), next.clone(), weak.clone());
                scope.spawn(move || loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    if i >= missing.len() {
                        break;
                    }
                    let r = waveform::fingerprint(&missing[i]).map_err(|e| e.to_string());
                    results.lock().unwrap().push((missing[i].clone(), r));
                    let n = done.fetch_add(1, Ordering::SeqCst) + 1;
                    let progress = n as f32 / total.max(1) as f32;
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        if with_state(|s| s.generation) == generation {
                            ui.set_analyze_progress(progress);
                            ui.set_status(format!("Analyzing audio… {n} of {total}").into());
                        }
                    });
                });
            }
        });
        let results = Arc::try_unwrap(results).map(|m| m.into_inner().unwrap()).unwrap_or_default();
        let _ = weak.upgrade_in_event_loop(move |ui| finish_analysis(&ui, generation, results));
    });
}

fn finish_analysis(ui: &AppWindow, generation: u64, results: Vec<(PathBuf, Result<waveform::Fingerprint, String>)>) {
    if with_state(|s| s.generation) != generation {
        return;
    }
    with_state(|s| {
        for (path, r) in results {
            // A file that fails to fingerprint (e.g. no audio stream) just can't get a waveform
            // match; it isn't a scan failure, so it doesn't go in `s.failures`.
            if let Ok(fp) = r {
                s.fingerprints.insert(path, fp);
            }
        }
    });
    let opts = sync_options(ui);
    with_state(|s| s.plan = plan(&s.files, &s.fingerprints, &opts));
    ui.set_analyzing(false);
    finish_planning(ui);
}

/// Refreshes everything that depends on `State::plan`, once it's up to date.
fn finish_planning(ui: &AppWindow) {
    refresh_media(ui);
    if !ui.get_scanning() && !ui.get_building() && !ui.get_analyzing() {
        update_scan_status(ui);
    }
    refresh_destination(ui);
}

fn update_scan_status(ui: &AppWindow) {
    let (timecode, waveform) = (ui.get_sync_timecode(), ui.get_sync_waveform());
    let (files, merged, failures, ffprobe_missing, summary) = with_state(|s| {
        (
            s.files.len(),
            s.plan.merged.len(),
            s.failures.len(),
            s.failures.iter().any(|(_, e)| e.contains("ffprobe not found")),
            media_summary(s),
        )
    });
    let ffmpeg_missing = waveform && probe::ffmpeg_path().is_none();
    if ffprobe_missing {
        set_status(ui, "ffprobe wasn't found. Install FFmpeg (brew install ffmpeg) to read video files.", 3);
    } else if ffmpeg_missing {
        set_status(ui, "ffmpeg wasn't found. Install FFmpeg (brew install ffmpeg) to sync by waveform.", 3);
    } else if files == 0 {
        set_status(ui, "No audio or video files found in these sources.", 2);
    } else if merged == 0 {
        let how = match (timecode, waveform) {
            (true, true) => "timecode or waveform",
            (_, true) => "waveform",
            _ => "timecode",
        };
        set_status(ui, &format!("No video matched any audio by {how}, so nothing can be synced."), 2);
    } else if failures > 0 {
        set_status(ui, &format!("{summary}  ·  {failures} unreadable"), 2);
    } else {
        set_status(ui, &summary, 0);
    }
}

fn media_summary(s: &State) -> String {
    let p = &s.plan;
    let mut parts = vec![format!("{} files", s.files.len() + s.failures.len()), format!("{} synced", p.merged.len())];
    if !p.multicam.is_empty() {
        parts.push(format!("{} multicam", p.multicam.len()));
    }
    if !p.unsynced.is_empty() {
        parts.push(format!("{} unmatched", p.unsynced.len()));
    }
    parts.join("  ·  ")
}

fn timecode(ticks: Option<i64>, rate: Option<Rate>) -> String {
    let Some(ticks) = ticks else { return "—".into() };
    let rate = rate.unwrap_or(Rate::new(24000, 1001));
    let fps = rate.nominal_fps();
    let frames = ticks / rate.frame_ticks();
    let (f, s) = (frames % fps, frames / fps);
    format!("{:02}:{:02}:{:02}:{:02}", s / 3600 % 24, s / 60 % 60, s % 60, f)
}

fn duration(ticks: i64) -> String {
    let secs = (ticks / TICKS_PER_SECOND).max(0);
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

fn refresh_media(ui: &AppWindow) {
    let waveform_sync = ui.get_sync_waveform();
    let rows = with_state(|s| {
        let mut rows = Vec::new();
        let p = &s.plan;
        for (i, f) in s.files.iter().enumerate() {
            let slate = match (&f.slate.scene, &f.slate.take) {
                (Some(scene), Some(take)) => format!("{scene}{} / {take}", f.slate.shot.clone().unwrap_or_default()),
                (Some(scene), None) => format!("{scene}{}", f.slate.shot.clone().unwrap_or_default()),
                _ => String::new(),
            };
            let multicam = p.multicam.iter().find(|g| g.angles.iter().flatten().any(|&v| v == i));
            let (status, tone, partner) = if f.start.is_none() && !waveform_sync {
                ("No timecode".to_string(), 3, String::new())
            } else if let Some(group) = multicam {
                let cam = f.camera.clone().unwrap_or_default();
                let audio = group.audio.map(|a| s.files[a].file_name()).unwrap_or_default();
                (format!("Multicam {cam}").trim().to_string(), 2, audio)
            } else if f.kind == MediaKind::Video {
                match p.merged.iter().find(|m| m.video == i) {
                    Some(m) => {
                        let how = if matches!(m.method, SyncMethod::Waveform { .. }) { "waveform" } else { "timecode" };
                        (format!("Synced ({how})"), 1, s.files[m.audio].file_name())
                    }
                    None => ("No match".into(), 3, String::new()),
                }
            } else {
                let videos: Vec<String> = p.merged.iter().filter(|m| m.audio == i).map(|m| s.files[m.video].file_name()).collect();
                let in_group = p.multicam.iter().any(|g| g.audio == Some(i));
                match (videos.len(), in_group) {
                    (0, _) => ("No match".into(), 3, String::new()),
                    (_, true) => ("Multicam".into(), 2, format!("{} angles", videos.len())),
                    (1, _) => ("Synced".into(), 1, videos[0].clone()),
                    (n, _) => ("Synced".into(), 1, format!("{n} videos")),
                }
            };
            rows.push(MediaRow {
                name: f.file_name().into(),
                is_video: f.kind == MediaKind::Video,
                timecode: timecode(f.start, f.rate).into(),
                duration: duration(f.duration).into(),
                slate: slate.into(),
                status: status.into(),
                tone,
                partner: partner.into(),
            });
        }
        for (path, error) in &s.failures {
            rows.push(MediaRow {
                name: display_name(path).into(),
                is_video: scan::classify(path) == Some(MediaKind::Video),
                timecode: "—".into(),
                duration: "".into(),
                slate: "".into(),
                status: "Unreadable".into(),
                tone: 4,
                partner: error.lines().next().unwrap_or("").into(),
            });
        }
        rows
    });
    ui.set_media(ModelRc::new(VecModel::from(rows)));
}

fn refresh_destination(ui: &AppWindow) {
    let (existing, output, merged) = with_state(|s| (s.existing.clone(), s.output.clone(), s.plan.merged.len()));
    match (&existing, ui.get_dest_mode()) {
        (Some((path, bins)), _) => {
            ui.set_project_name(path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default().into());
            ui.set_project_detail(
                match bins.len() {
                    0 => "No bins".to_string(),
                    1 => "1 bin".to_string(),
                    n => format!("{n} bins"),
                }
                .into(),
            );
            let mut model: Vec<SharedString> = vec!["Project root".into(), "New bin…".into()];
            model.extend(bins.iter().map(|b| format!("{}{}", "   ".repeat(b.depth), b.name).into()));
            let current = ui.get_bins();
            let changed = current.row_count() != model.len()
                || current.iter().zip(model.iter()).any(|(a, b)| &a != b);
            if changed {
                ui.set_bins(ModelRc::new(VecModel::from(model)));
                ui.set_bin_index(1);
            }
        }
        (None, _) => {
            ui.set_project_name("".into());
            ui.set_project_detail("".into());
        }
    }
    let output_name = match ui.get_dest_mode() {
        0 | 2 => output.as_ref().map(|p| display_name(p)).unwrap_or_default(),
        _ => output
            .as_ref()
            .or(existing.as_ref().map(|e| &e.0))
            .map(|p| display_name(p))
            .unwrap_or_default(),
    };
    ui.set_output_name(output_name.into());
    let has_target = ui.get_dest_mode() != 1 || existing.is_some();
    ui.set_can_build(merged > 0 && has_target);
}

fn open_existing(ui: &AppWindow, path: PathBuf) {
    set_status(ui, &format!("Opening {}…", display_name(&path)), 0);
    let weak: Weak<AppWindow> = ui.as_weak();
    std::thread::spawn(move || {
        let result = Project::open(&path).map(|p| p.bins());
        let _ = weak.upgrade_in_event_loop(move |ui| match result {
            Ok(bins) => {
                with_state(|s| {
                    s.existing = Some((path.clone(), bins));
                    s.output = None;
                });
                ui.set_dest_mode(1);
                refresh_destination(&ui);
                set_status(&ui, "Close this project in Premiere before adding clips. A backup is saved next to it.", 0);
            }
            Err(e) => set_status(&ui, &format!("Couldn't open {}: {e}", display_name(&path)), 3),
        });
    });
}

fn ask_output(ui: &AppWindow) -> Option<PathBuf> {
    let xml = ui.get_dest_mode() == 2;
    let ext = if xml { "xml" } else { "prproj" };
    let (dir, name) = with_state(|s| {
        let base = s.existing.as_ref().map(|e| e.0.clone()).filter(|_| ui.get_dest_mode() == 1);
        match base {
            Some(p) => (p.parent().map(Path::to_path_buf), display_name(&p)),
            None => {
                let first = s.sources.first().cloned();
                let dir = first.as_ref().map(|p| if p.is_dir() { p.clone() } else { p.parent().unwrap_or(p).to_path_buf() });
                let stem = dir.as_ref().map(|d| display_name(d)).unwrap_or_else(|| "Synced".into());
                let name = if xml { format!("{stem} Synced.xml") } else { format!("{stem}.prproj") };
                (dir.and_then(|d| d.parent().map(Path::to_path_buf)), name)
            }
        }
    });
    let (title, filter) = if xml {
        ("Save XML timeline", ("Final Cut Pro XML", &["xml"][..]))
    } else {
        ("Save Premiere project", ("Premiere Pro Project", &["prproj"][..]))
    };
    dialogs::save_file(title, dir.as_deref(), &name, filter).map(|p| {
        if p.extension().is_some_and(|e| e == ext) {
            p
        } else {
            p.with_extension(ext)
        }
    })
}

fn start_build(ui: &AppWindow) {
    let mode = ui.get_dest_mode();
    let output = match (with_state(|s| s.output.clone()), mode) {
        (Some(p), _) => p,
        (None, 1) => match with_state(|s| s.existing.as_ref().map(|e| e.0.clone())) {
            Some(p) => p,
            None => return,
        },
        (None, _) => match ask_output(ui) {
            Some(p) => {
                with_state(|s| s.output = Some(p.clone()));
                refresh_destination(ui);
                p
            }
            None => return,
        },
    };
    let opts = BuildOptions {
        rename_from_metadata: ui.get_rename(),
        name_template: ui.get_name_template().to_string(),
        scene_bins: ui.get_scene_bins(),
        shot_bins: ui.get_shot_bins(),
        multicam: ui.get_multicam(),
        trim_to_overlap: ui.get_trim(),
        import_unsynced: ui.get_import_unsynced(),
        timeline: match ui.get_timeline() {
            2 => Timeline::PerTake,
            0 if mode != 2 => Timeline::None,
            _ => Timeline::All,
        },
        scene_stringouts: ui.get_scene_stringouts(),
        day_sequences: ui.get_day_sequences(),
    };
    let existing = if mode == 1 { with_state(|s| s.existing.clone()) } else { None };
    let bin_index = ui.get_bin_index();
    let new_bin = ui.get_new_bin_name().trim().to_string();
    let (files, sync_plan) = with_state(|s| (s.files.clone(), s.plan.clone()));

    ui.set_building(true);
    ui.set_can_reveal(false);
    set_status(ui, if mode == 2 { "Writing XML…" } else { "Building project…" }, 0);
    let weak = ui.as_weak();
    std::thread::spawn(move || {
        let result = (|| -> anyhow::Result<premiere_sound_sync_core::BuildReport> {
            let name = output.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if mode == 2 {
                let (xml, report) = build_xml(&files, &sync_plan, &name, &opts);
                std::fs::write(&output, xml)?;
                return Ok(report);
            }
            let mut project = match &existing {
                Some((path, _)) => Project::open(path)?,
                None => Project::new_blank()?,
            };
            let root = project.root().to_string();
            let new_bin = if new_bin.is_empty() { "Synced".to_string() } else { new_bin };
            let (target, timeline_name) = match (&existing, bin_index) {
                (None, _) | (Some(_), 0) => (root, name),
                (Some(_), 1) => (project.ensure_bin(&root, &new_bin), new_bin),
                (Some((_, bins)), i) => match bins.get(i as usize - 2) {
                    Some(b) => (b.uid.clone(), b.name.clone()),
                    None => (root, name),
                },
            };
            let report = build(&files, &sync_plan, &mut project, &target, &timeline_name, &opts);
            project.save(&output)?;
            Ok(report)
        })();
        let _ = weak.upgrade_in_event_loop(move |ui| {
            ui.set_building(false);
            match result {
                Ok(report) => {
                    with_state(|s| s.last_output = Some(output.clone()));
                    ui.set_can_reveal(true);
                    let sequences = match report.sequences {
                        1 => "1 sequence".to_string(),
                        n => format!("{n} sequences"),
                    };
                    let mut parts = if mode == 2 {
                        vec![format!("{} synced takes", report.takes)]
                    } else {
                        vec![format!("{} merged clips", report.merged_clips)]
                    };
                    if report.sequences > 0 {
                        parts.push(sequences);
                    }
                    if report.plain_clips > 0 {
                        parts.push(format!("{} unsynced clips", report.plain_clips));
                    }
                    let mut text = format!("Saved {}: {}.", display_name(&output), parts.join(", "));
                    if let Some(w) = report.warnings.first() {
                        text.push(' ');
                        text.push_str(w);
                    }
                    set_status(&ui, &text, if report.warnings.is_empty() { 1 } else { 2 });
                    if let Some((path, _)) = with_state(|s| s.existing.clone()) {
                        if let Ok(p) = Project::open(&path) {
                            with_state(|s| s.existing = Some((path, p.bins())));
                            refresh_destination(&ui);
                        }
                    }
                }
                Err(e) => set_status(&ui, &format!("Build failed: {e}"), 3),
            }
        });
    });
}
