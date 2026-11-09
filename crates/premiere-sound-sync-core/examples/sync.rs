//! Headless run: `cargo run -p premiere-sound-sync-core --example sync -- OUT.prproj|OUT.xml [--into EXISTING.prproj] SOURCES...`

use std::path::PathBuf;

use anyhow::{bail, Result};
use premiere_sound_sync_core::{build, build_xml, plan, probe, scan, BuildOptions, Project};

fn main() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        bail!("usage: sync OUT.prproj [--into EXISTING.prproj] SOURCES...");
    }
    let out = PathBuf::from(args.remove(0));
    let mut existing = None;
    if args.first().map(String::as_str) == Some("--into") {
        args.remove(0);
        existing = Some(PathBuf::from(args.remove(0)));
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
    let opts = BuildOptions::default();
    let sync_plan = plan(&files, opts.multicam);
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
