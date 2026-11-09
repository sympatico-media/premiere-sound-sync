//! Headless run: `cargo run -p premiere-sound-sync-core --example sync -- OUT.prproj|OUT.xml [--into EXISTING.prproj] [--xml-primary all|per-take] [--xml-scene-stringouts] [--xml-day-sequences] SOURCES...`
//!
//! The `--xml-*` flags only affect `OUT.xml`; a `.prproj` destination always writes merged
//! clips (see `BuildOptions::xml_primary` for why per-take/multicam sequences are XML-only).

use std::path::PathBuf;

use anyhow::{bail, Result};
use premiere_sound_sync_core::{build, build_xml, plan, probe, scan, BuildOptions, Project, SyncOptions, XmlPrimary};

fn main() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        bail!(
            "usage: sync OUT.prproj|OUT.xml [--into EXISTING.prproj] [--xml-primary all|per-take] \
             [--xml-scene-stringouts] [--xml-day-sequences] SOURCES..."
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
            "--xml-primary" => {
                args.remove(i);
                if i >= args.len() {
                    bail!("--xml-primary needs 'all' or 'per-take'");
                }
                let value = args.remove(i);
                opts.xml_primary = match value.as_str() {
                    "all" => XmlPrimary::All,
                    "per-take" => XmlPrimary::PerTake,
                    other => bail!("--xml-primary: unknown value '{other}' (want 'all' or 'per-take')"),
                };
            }
            "--xml-scene-stringouts" => {
                args.remove(i);
                opts.xml_scene_stringouts = true;
            }
            "--xml-day-sequences" => {
                args.remove(i);
                opts.xml_day_sequences = true;
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
    if out.extension().is_some_and(|e| e.eq_ignore_ascii_case("xml")) {
        let name = out.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
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
    let report = build(&files, &sync_plan, &mut project, &target, &opts);
    project.save(&out)?;
    println!("{report:#?}\nwrote {}", out.display());
    Ok(())
}
