//! Headless run: `cargo run -p premiere-sound-sync-core --example sync -- OUT.prproj|OUT.xml [--into EXISTING.prproj] [--timeline none|all|per-take] [--scene-stringouts] [--day-sequences] SOURCES...`

use std::path::PathBuf;

use anyhow::{bail, Result};
use premiere_sound_sync_core::{build, build_xml, plan, probe, scan, BuildOptions, Project, SyncOptions, Timeline};

fn main() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        bail!(
            "usage: sync OUT.prproj|OUT.xml [--into EXISTING.prproj] [--timeline none|all|per-take] \
             [--scene-stringouts] [--day-sequences] SOURCES..."
        );
    }
    let out = PathBuf::from(args.remove(0));
    let mut existing = None;
    let mut opts = BuildOptions::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--into" => {
                args.remove(i);
                if i >= args.len() {
                    bail!("--into needs a path");
                }
                existing = Some(PathBuf::from(args.remove(i)));
            }
            "--timeline" => {
                args.remove(i);
                if i >= args.len() {
                    bail!("--timeline needs 'none', 'all' or 'per-take'");
                }
                let value = args.remove(i);
                opts.timeline = match value.as_str() {
                    "none" => Timeline::None,
                    "all" => Timeline::All,
                    "per-take" => Timeline::PerTake,
                    other => bail!("--timeline: unknown value '{other}' (want 'none', 'all' or 'per-take')"),
                };
            }
            "--scene-stringouts" => {
                args.remove(i);
                opts.scene_stringouts = true;
            }
            "--day-sequences" => {
                args.remove(i);
                opts.day_sequences = true;
            }
            _ => i += 1,
        }
    }
    let sources: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
    let paths = scan::collect(&sources);
    println!("found {} media files", paths.len());
    let mut files = Vec::new();
    for p in &paths {
        match probe::probe(p) {
            Ok(f) => files.push(f),
            Err(e) => println!("  skip {}: {e}", p.display()),
        }
    }
    let sync_opts = SyncOptions { multicam: opts.multicam, ..SyncOptions::default() };
    let sync_plan = plan(&files, &std::collections::HashMap::new(), &sync_opts);
    for m in &sync_plan.merged {
        println!("  {} + {}", files[m.video].file_name(), files[m.audio].file_name());
    }
    let name = out.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if out.extension().is_some_and(|e| e.eq_ignore_ascii_case("xml")) {
        let (xml, report) = build_xml(&files, &sync_plan, &name, &opts);
        std::fs::write(&out, xml)?;
        println!("{report:#?}\nwrote {}", out.display());
        return Ok(());
    }
    let mut project = match &existing {
        Some(p) => Project::open(p)?,
        None => Project::new_blank()?,
    };
    let root = project.root().to_string();
    let target = project.ensure_bin(&root, "Synced");
    let report = build(&files, &sync_plan, &mut project, &target, &name, &opts);
    project.save(&out)?;
    println!("{report:#?}\nwrote {}", out.display());
    Ok(())
}
