use std::process::Command;

const LATEST_RELEASE: &str = "https://api.github.com/repos/sympatico-media/premiere-sound-sync/releases/latest";

pub struct Release {
    pub version: String,
    pub url: String,
}

/// Returns the latest GitHub release if it's newer than this build. Any network or parse
/// failure returns `None`; being offline should never bother the user.
pub fn check() -> Option<Release> {
    check_against(env!("CARGO_PKG_VERSION"))
}

fn check_against(current: &str) -> Option<Release> {
    let output = Command::new("curl")
        .args(["-fsSL", "--max-time", "10", "-H", "Accept: application/vnd.github+json"])
        .arg(LATEST_RELEASE)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let tag = json.get("tag_name")?.as_str()?;
    let version = tag.trim_start_matches('v');
    if parse(version)? <= parse(current)? {
        return None;
    }
    Some(Release {
        version: version.to_string(),
        url: json.get("html_url")?.as_str()?.to_string(),
    })
}

fn parse(version: &str) -> Option<Vec<u64>> {
    version.split('.').map(|p| p.parse().ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::{check_against, parse};

    #[test]
    #[ignore = "hits the network"]
    fn finds_latest_release() {
        let release = check_against("0.0.1").expect("GitHub should report a newer release");
        assert!(release.url.contains("/releases/tag/"));
        assert!(check_against("999.0.0").is_none());
    }

    #[test]
    fn compares_numerically() {
        assert!(parse("0.10.0") > parse("0.9.3"));
        assert!(parse("1.0.0") > parse("0.99.0"));
        assert_eq!(parse("0.2.0"), parse("0.2.0"));
        assert_eq!(parse("0.2.0-beta"), None);
    }
}
