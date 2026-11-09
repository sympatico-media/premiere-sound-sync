//! Core of Premiere Sync: find media, read timecode and slate metadata, pair video with
//! production audio, and write merged clips / bins into Premiere Pro projects.

pub mod build;
pub mod bwf;
pub mod media;
pub mod naming;
pub mod prproj;
pub mod probe;
pub mod scan;
pub mod sync;
pub mod time;
pub mod xmeml;

pub use build::{build, build_xml, BuildOptions, BuildReport};
pub use media::{MediaFile, MediaKind, Slate};
pub use prproj::{BinInfo, Project};
pub use sync::{plan, SyncPlan};
