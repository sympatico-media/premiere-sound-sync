//! Clip naming from slate metadata (scene / shot / take / circle).

use std::sync::OnceLock;

use regex::Regex;

use crate::media::Slate;

pub const DEFAULT_TEMPLATE: &str = "{scene}{shot}-{take}{circle}";

/// Splits a scene field like `12A` into scene `12` and shot `A`.
pub fn split_scene(raw: &str) -> (Option<String>, Option<String>) {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^\s*([0-9]+)[\s._-]*([A-Za-z]{1,3})?\s*$").unwrap());
    let raw = raw.trim();
    if raw.is_empty() {
        return (None, None);
    }
    match re.captures(raw) {
        Some(c) => (
            Some(c[1].to_string()),
            c.get(2).map(|m| m.as_str().to_uppercase()),
        ),
        None => (Some(raw.to_string()), None),
    }
}

/// Best-effort slate from a file name such as `2A-001`, `S12_SH4_TK2`, or `12B_T3`.
pub fn slate_from_filename(stem: &str) -> Slate {
    static EXPLICIT: OnceLock<Regex> = OnceLock::new();
    static COMPACT: OnceLock<Regex> = OnceLock::new();
    let explicit = EXPLICIT.get_or_init(|| {
        Regex::new(r"(?i)(?:^|[^a-z])s(?:c|cene)?[\s._-]*(\d+)([a-z]?)(?:[\s._-]*(?:sh|shot)[\s._-]*([a-z0-9]+))?[\s._-]*(?:t|tk|take)[\s._-]*(\d+)").unwrap()
    });
    let compact = COMPACT.get_or_init(|| {
        Regex::new(r"(?i)^(\d+)([a-z]{0,2})[\s._-]+(?:t|tk|take)?[\s._-]*(\d{1,4})$").unwrap()
    });
    if let Some(c) = explicit.captures(stem) {
        let shot = c
            .get(3)
            .or(c.get(2).filter(|m| !m.as_str().is_empty()))
            .map(|m| m.as_str().to_uppercase());
        return Slate {
            scene: Some(c[1].to_string()),
            shot,
            take: Some(c[4].to_string()),
            circled: false,
        };
    }
    if let Some(c) = compact.captures(stem) {
        return Slate {
            scene: Some(c[1].to_string()),
            shot: c.get(2).filter(|m| !m.as_str().is_empty()).map(|m| m.as_str().to_uppercase()),
            take: Some(c[3].to_string()),
            circled: false,
        };
    }
    Slate::default()
}

/// Renders a name template. Tokens: `{scene} {shot} {take} {circle} {camera} {clip}`.
/// Falls back to `fallback` when the slate has no scene or take.
pub fn render(template: &str, slate: &Slate, camera: Option<&str>, fallback: &str) -> String {
    if slate.scene.is_none() && slate.take.is_none() {
        return fallback.to_string();
    }
    let name = template
        .replace("{scene}", slate.scene.as_deref().unwrap_or(""))
        .replace("{shot}", slate.shot.as_deref().unwrap_or(""))
        .replace("{take}", slate.take.as_deref().unwrap_or(""))
        .replace("{circle}", if slate.circled { " *" } else { "" })
        .replace("{camera}", camera.unwrap_or(""))
        .replace("{clip}", fallback);
    let name = name.trim().trim_matches(|c| c == '-' || c == '_').trim().to_string();
    if name.is_empty() {
        fallback.to_string()
    } else {
        name
    }
}

pub fn scene_bin_name(slate: &Slate) -> Option<String> {
    slate.scene.as_ref().map(|s| format!("Scene {s}"))
}

pub fn shot_bin_name(slate: &Slate) -> Option<String> {
    let scene = slate.scene.as_deref()?;
    let shot = slate.shot.as_deref()?;
    Some(format!("{scene}{shot}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_scene_and_shot() {
        assert_eq!(split_scene("2A"), (Some("2".into()), Some("A".into())));
        assert_eq!(split_scene("12"), (Some("12".into()), None));
        assert_eq!(split_scene("Intro"), (Some("Intro".into()), None));
    }

    #[test]
    fn parses_filenames() {
        let s = slate_from_filename("2A-001");
        assert_eq!((s.scene.as_deref(), s.shot.as_deref(), s.take.as_deref()), (Some("2"), Some("A"), Some("001")));
        let s = slate_from_filename("S12_SH4_TK2");
        assert_eq!((s.scene.as_deref(), s.shot.as_deref(), s.take.as_deref()), (Some("12"), Some("4"), Some("2")));
        let s = slate_from_filename("12B_T3");
        assert_eq!((s.scene.as_deref(), s.shot.as_deref(), s.take.as_deref()), (Some("12"), Some("B"), Some("3")));
        assert!(slate_from_filename("A001C001_260910_C5F0").is_empty());
    }

    #[test]
    fn renders_template() {
        let slate = Slate {
            scene: Some("2".into()),
            shot: Some("A".into()),
            take: Some("001".into()),
            circled: true,
        };
        assert_eq!(render(DEFAULT_TEMPLATE, &slate, None, "clip"), "2A-001 *");
        assert_eq!(render(DEFAULT_TEMPLATE, &Slate::default(), None, "A001C001"), "A001C001");
        let no_shot = Slate { shot: None, circled: false, ..slate };
        assert_eq!(render(DEFAULT_TEMPLATE, &no_shot, None, "x"), "2-001");
    }
}
