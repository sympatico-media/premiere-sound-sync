# Premiere Sync

Premiere Sync pairs production audio with camera video by timecode and builds a Premiere Pro project from the result. Drop in a shoot folder and it finds the media, matches picture to sound, and writes merged clips, scene bins, and multicam sequences.

Clip names inside Premiere can come from slate metadata. Files on disk keep their original names.

![Premiere Sync, matching a shoot folder by timecode](docs/premiere-sync.png)

Synced clips show up in Premiere named from the slate, grouped into a bin per scene.

![Scene 2 in Premiere, with merged clips named from the slate](docs/premiere-bin.png)

## What it writes

- **New project.** A `.prproj` with synced clips in a `Synced` bin.
- **Existing project.** The same clips added to a bin you choose.
- **XML timeline.** An interchange timeline for Premiere Pro or DaVinci Resolve. XML has no bins, multicam sequences, or unsynced clips.

Merged clips are trimmed to the stretch where picture and sound both exist. Files that never match can still be imported as plain clips.

## How matching works

Each file needs a start timecode. WAV, BWF, and RF64 audio are read directly from the Broadcast WAV `bext` and `iXML` chunks. Everything else is read with `ffprobe`, which ships inside the macOS app.

A video clip pairs with the audio file that overlaps it the most. The overlap has to be at least one second, or half the shorter clip when the clip is very short. Multicam groups angles that share that production audio, or that overlap each other and carry different camera ids.

Slate (scene, shot, take, circled take) comes from recorder metadata, camera tags such as ARRI scene and take, or the file name (`12A-3`, `S12_T3`). The default clip name is `{scene}{shot}-{take}{circle}`. Other tokens are `{camera}` and `{clip}`.

## Download

Get the Apple Silicon build from [Releases](https://github.com/sympatico-media/premiere-sound-sync/releases). It includes `ffprobe`, so nothing else needs to be installed.

## Building from source

- A recent stable [Rust](https://rustup.rs/) toolchain
- [FFmpeg](https://ffmpeg.org/) for `cargo run`, so `ffprobe` is on `PATH` (or in `/opt/homebrew/bin` or `/usr/local/bin` on macOS)

## Run

```sh
cargo run -p premiere-sound-sync
```

On macOS, this builds a `Premiere Sync.app` bundle:

```sh
scripts/bundle-macos.sh
```

Pass `debug` to bundle the debug build. The app is written to `target/release/Premiere Sync.app`. The script downloads a static arm64 `ffprobe` from [martin-riedl.de](https://ffmpeg.martin-riedl.de), checks its SHA-256, and puts it in `Contents/Resources`. Set `FFPROBE=/path/to/ffprobe` to bundle a different build.

## Headless

The core crate can sync without the window. The first argument is the output path; everything after it is a file or folder to scan.

```sh
cargo run -p premiere-sound-sync-core --example sync -- episode.prproj /path/to/shoot
cargo run -p premiere-sound-sync-core --example sync -- episode.prproj --into existing.prproj /path/to/shoot
cargo run -p premiere-sound-sync-core --example sync -- timeline.xml /path/to/shoot
```

## Layout

| Path | Role |
| --- | --- |
| `apps/premiere-sound-sync` | Slint desktop app |
| `crates/premiere-sound-sync-core` | Scan, probe, sync, and Premiere / XML writing |
| `crates/studio-ui` | Shared interface components |
| `scripts/bundle-macos.sh` | macOS app bundle |

## License

MIT. The bundled `ffprobe` is part of [FFmpeg](https://ffmpeg.org/) and is licensed under the GPLv3. Its source is available from [ffmpeg.org](https://ffmpeg.org/download.html) and the build scripts from [martin-riedl.de](https://git.martin-riedl.de/ffmpeg/build-script).
