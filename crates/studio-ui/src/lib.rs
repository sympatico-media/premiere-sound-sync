//! Studio UI: a small, reusable Slint component kit for dark macOS-style utility apps.
//!
//! In a dependent app's `build.rs`:
//!
//! ```ignore
//! let mut paths = std::collections::HashMap::new();
//! paths.insert("studio".to_string(), std::env::var("DEP_STUDIO_UI_LIBRARY").unwrap().into());
//! slint_build::compile_with_config("ui/app.slint",
//!     slint_build::CompilerConfiguration::new().with_library_paths(paths).with_style("cupertino-dark".into())).unwrap();
//! ```
//!
//! Then `import { Theme, Pane, DropZone, GroupBox, PrimaryButton } from "@studio";` in `.slint` files.

use std::path::PathBuf;

use slint::winit_030::winit::event::WindowEvent;
use slint::winit_030::{EventResult, WinitWindowAccessor};

/// Selects the winit backend with the Skia renderer (closest to native text) and gives macOS windows a transparent, full-height title bar
/// so panes run edge to edge under the traffic lights.
pub fn init_backend() -> Result<(), slint::PlatformError> {
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("skia".into())
        .with_winit_window_attributes_hook(|attrs| {
            #[cfg(target_os = "macos")]
            {
                use slint::winit_030::winit::platform::macos::WindowAttributesExtMacOS;
                attrs
                    .with_titlebar_transparent(true)
                    .with_fullsize_content_view(true)
                    .with_title_hidden(true)
            }
            #[cfg(not(target_os = "macos"))]
            {
                attrs
            }
        })
        .select()
}

/// Sets the name shown in the menu bar, Dock and app switcher when running outside an app bundle.
/// Call before creating the first window.
pub fn set_app_name(name: &str) {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSProcessInfo, NSString};
        NSProcessInfo::processInfo().setProcessName(&NSString::from_str(name));
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = name;
    }
}

/// Shows `png` as the Dock and app-switcher icon. No-op on other platforms.
/// Call from the UI thread, after the first window exists.
pub fn set_app_icon(png: &[u8]) {
    #[cfg(target_os = "macos")]
    {
        use objc2::rc::Retained;
        use objc2::{AnyThread, MainThreadMarker};
        use objc2_app_kit::{NSApplication, NSImage};
        use objc2_foundation::NSData;

        let data: Retained<NSData> = NSData::with_bytes(png);
        let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
            return;
        };
        let app = NSApplication::sharedApplication(unsafe { MainThreadMarker::new_unchecked() });
        unsafe { app.setApplicationIconImage(Some(&image)) };
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = png;
    }
}

/// File drag and drop events from the OS.
#[derive(Debug, Clone)]
pub enum FileDrop {
    Hovering,
    Cancelled,
    Dropped(PathBuf),
}

/// Calls `handler` for files dragged over or dropped onto `window`.
pub fn on_file_drop(window: &slint::Window, mut handler: impl FnMut(FileDrop) + 'static) {
    window.on_winit_window_event(move |_, event| {
        match event {
            WindowEvent::HoveredFile(_) => handler(FileDrop::Hovering),
            WindowEvent::HoveredFileCancelled => handler(FileDrop::Cancelled),
            WindowEvent::DroppedFile(path) => handler(FileDrop::Dropped(path.clone())),
            _ => return EventResult::Propagate,
        }
        EventResult::Propagate
    });
}

/// Native pickers. They block, so call them from UI callbacks only.
pub mod dialogs {
    use std::path::{Path, PathBuf};

    pub fn pick_folders(title: &str) -> Vec<PathBuf> {
        rfd::FileDialog::new().set_title(title).pick_folders().unwrap_or_default()
    }

    pub fn pick_files(title: &str, filter: Option<(&str, &[&str])>) -> Vec<PathBuf> {
        let mut d = rfd::FileDialog::new().set_title(title);
        if let Some((name, exts)) = filter {
            d = d.add_filter(name, exts);
        }
        d.pick_files().unwrap_or_default()
    }

    pub fn pick_file(title: &str, filter: Option<(&str, &[&str])>) -> Option<PathBuf> {
        let mut d = rfd::FileDialog::new().set_title(title);
        if let Some((name, exts)) = filter {
            d = d.add_filter(name, exts);
        }
        d.pick_file()
    }

    pub fn save_file(title: &str, directory: Option<&Path>, file_name: &str, filter: (&str, &[&str])) -> Option<PathBuf> {
        let mut d = rfd::FileDialog::new()
            .set_title(title)
            .set_file_name(file_name)
            .add_filter(filter.0, filter.1);
        if let Some(dir) = directory {
            d = d.set_directory(dir);
        }
        d.save_file()
    }
}

/// Native file icons, as Finder draws them. Call from the UI thread.
/// `None` on platforms without system icons; fall back to a bundled image there.
pub mod system_icons {
    /// Icon for a file type: an extension (`"prproj"`) or a UTI (`"public.data"` is a blank document).
    pub fn for_file_type(file_type: &str, points: u32) -> Option<slint::Image> {
        #[cfg(target_os = "macos")]
        {
            use objc2_app_kit::NSWorkspace;
            use objc2_foundation::NSString;
            #[allow(deprecated)]
            let icon = NSWorkspace::sharedWorkspace().iconForFileType(&NSString::from_str(file_type));
            mac::rasterize(&icon, points)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (file_type, points);
            None
        }
    }

    /// Icon for a specific file, including custom icons set in Finder.
    pub fn for_path(path: &std::path::Path, points: u32) -> Option<slint::Image> {
        #[cfg(target_os = "macos")]
        {
            use objc2_app_kit::NSWorkspace;
            use objc2_foundation::NSString;
            let icon = NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(&path.to_string_lossy()));
            mac::rasterize(&icon, points)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (path, points);
            None
        }
    }

    #[cfg(target_os = "macos")]
    mod mac {
        use objc2::AnyThread;
        use objc2_app_kit::{NSBitmapImageRep, NSDeviceRGBColorSpace, NSGraphicsContext, NSImage};
        use objc2_foundation::{NSPoint, NSRect, NSSize};
        use slint::{Rgba8Pixel, SharedPixelBuffer};

        /// Draws `icon` into an RGBA bitmap at 2x so it stays sharp on Retina displays.
        pub fn rasterize(icon: &NSImage, points: u32) -> Option<slint::Image> {
            let px = (points * 2) as isize;
            let rep = unsafe {
                NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                    NSBitmapImageRep::alloc(),
                    std::ptr::null_mut(),
                    px,
                    px,
                    8,
                    4,
                    true,
                    false,
                    NSDeviceRGBColorSpace,
                    px * 4,
                    32,
                )
            }?;
            let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
            NSGraphicsContext::saveGraphicsState_class();
            NSGraphicsContext::setCurrentContext(Some(&context));
            icon.drawInRect(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(px as f64, px as f64)));
            context.flushGraphics();
            NSGraphicsContext::restoreGraphicsState_class();

            let stride = rep.bytesPerRow() as usize;
            let data = rep.bitmapData();
            if data.is_null() {
                return None;
            }
            let side = px as u32;
            let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(side, side);
            let pixels = buffer.make_mut_bytes();
            for row in 0..side as usize {
                let src = unsafe { std::slice::from_raw_parts(data.add(row * stride), side as usize * 4) };
                pixels[row * side as usize * 4..(row + 1) * side as usize * 4].copy_from_slice(src);
            }
            Some(slint::Image::from_rgba8_premultiplied(buffer))
        }
    }
}

/// Opens Finder (or the platform file manager) with `path` selected.
pub fn reveal_in_file_manager(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open")
        .arg(path.parent().unwrap_or(path))
        .spawn();
}

/// Opens `url` in the default browser.
pub fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}
