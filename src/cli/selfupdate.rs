use crate::error::{ZlError, ZlResult};

/// GitHub repository for ZL releases
const GITHUB_REPO: &str = "polpo21/zero_layer";

/// Handle `zl self-update`: download and replace the current binary with the latest release.
pub fn handle() -> ZlResult<()> {
    println!("Checking for updates...");

    let current_version = env!("CARGO_PKG_VERSION");
    println!("Current version: {}", current_version);

    // Get the path of the currently running binary
    let current_exe = std::env::current_exe().map_err(|e| {
        ZlError::SelfUpdate(format!("Cannot determine current executable path: {}", e))
    })?;

    // Resolve symlinks to get the real path
    let real_exe = std::fs::canonicalize(&current_exe).unwrap_or(current_exe.clone());
    tracing::debug!("Current binary: {}", real_exe.display());

    // Check if we can write to the binary location
    if let Some(parent) = real_exe.parent() {
        let metadata = std::fs::metadata(parent).map_err(|e| {
            ZlError::SelfUpdate(format!(
                "Cannot access binary directory {}: {}",
                parent.display(),
                e
            ))
        })?;
        if metadata.permissions().readonly() {
            return Err(ZlError::SelfUpdate(format!(
                "Binary directory {} is not writable. Try running with appropriate permissions.",
                parent.display()
            )));
        }
    }

    // Replace the binary with the same build flavour that is running now:
    // a musl build must not be swapped for a glibc one.
    let target = release_target().ok_or_else(|| {
        ZlError::SelfUpdate(format!(
            "No prebuilt release for this platform ({}-{})",
            std::env::consts::ARCH,
            if cfg!(target_env = "musl") {
                "musl"
            } else {
                "gnu"
            }
        ))
    })?;

    // Fetch latest release info from GitHub API
    let api_url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        GITHUB_REPO
    );

    let client = reqwest::blocking::Client::builder()
        .user_agent("zl-self-update")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| ZlError::SelfUpdate(format!("HTTP client error: {}", e)))?;

    let response = client
        .get(&api_url)
        .send()
        .map_err(|e| ZlError::SelfUpdate(format!("Failed to check for updates: {}", e)))?;

    if !response.status().is_success() {
        let msg = if response.status().as_u16() == 404 {
            "No releases found on GitHub — check that the repository has published releases"
                .to_string()
        } else {
            format!(
                "GitHub API returned status {}: check your internet connection or try again later",
                response.status()
            )
        };
        return Err(ZlError::SelfUpdate(msg));
    }

    let body_text = response
        .text()
        .map_err(|e| ZlError::SelfUpdate(format!("Failed to read response: {}", e)))?;
    let body: serde_json::Value = serde_json::from_str(&body_text)
        .map_err(|e| ZlError::SelfUpdate(format!("Failed to parse release info: {}", e)))?;

    let latest_version = body["tag_name"]
        .as_str()
        .ok_or_else(|| ZlError::SelfUpdate("No tag_name in release".into()))?
        .trim_start_matches('v');

    if !is_newer(latest_version, current_version) {
        println!("Already at the latest version ({}).", current_version);
        return Ok(());
    }

    println!(
        "New version available: {} -> {}",
        current_version, latest_version
    );

    let assets = body["assets"]
        .as_array()
        .ok_or_else(|| ZlError::SelfUpdate("No assets in release".into()))?;

    let asset_name = format!("zl-{}", target);
    let download_url = asset_url(assets, &asset_name).ok_or_else(|| {
        ZlError::SelfUpdate(format!(
            "No binary found for {} in release {}. Available assets: {}",
            target,
            latest_version,
            assets
                .iter()
                .filter_map(|a| a["name"].as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })?;
    let sums_url = asset_url(assets, "SHA256SUMS.txt").ok_or_else(|| {
        ZlError::SelfUpdate(format!(
            "Release {} has no SHA256SUMS.txt; refusing to install an unverified binary",
            latest_version
        ))
    })?;

    println!("Downloading {}...", download_url);

    let binary_bytes = client
        .get(download_url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| ZlError::SelfUpdate(format!("Download failed: {}", e)))?
        .bytes()
        .map_err(|e| ZlError::SelfUpdate(format!("Failed to read download: {}", e)))?;

    let sums = client
        .get(sums_url)
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.text())
        .map_err(|e| ZlError::SelfUpdate(format!("Failed to download SHA256SUMS.txt: {}", e)))?;
    let expected = expected_checksum(&sums, &asset_name).ok_or_else(|| {
        ZlError::SelfUpdate(format!("SHA256SUMS.txt has no entry for {}", asset_name))
    })?;
    let actual = crate::core::verify::sha256_hex(&binary_bytes);
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(ZlError::SelfUpdate(format!(
            "Checksum mismatch for {}: expected {}, got {}",
            asset_name, expected, actual
        )));
    }
    println!("Checksum verified.");

    // Write to a temp file first, then atomically replace
    let tmp_path = real_exe.with_extension("update-tmp");
    std::fs::write(&tmp_path, &binary_bytes)
        .map_err(|e| ZlError::SelfUpdate(format!("Failed to write update file: {}", e)))?;

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| ZlError::SelfUpdate(format!("Failed to set permissions: {}", e)))?;
    }

    // Atomic rename (same filesystem)
    std::fs::rename(&tmp_path, &real_exe).map_err(|e| {
        // Clean up temp file on failure
        let _ = std::fs::remove_file(&tmp_path);
        ZlError::SelfUpdate(format!(
            "Failed to replace binary: {}. You may need to run with elevated permissions.",
            e
        ))
    })?;

    println!("Updated to {} successfully!", latest_version);
    println!("Restart zl to use the new version.");

    Ok(())
}

/// The release asset suffix matching the running binary, as published by
/// `.github/workflows/release.yml`.
fn release_target() -> Option<&'static str> {
    let musl = cfg!(target_env = "musl");
    match (std::env::consts::ARCH, musl) {
        ("x86_64", false) => Some("x86_64-unknown-linux-gnu"),
        ("x86_64", true) => Some("x86_64-unknown-linux-musl"),
        ("aarch64", false) => Some("aarch64-unknown-linux-gnu"),
        ("aarch64", true) => Some("aarch64-unknown-linux-musl"),
        _ => None,
    }
}

/// Download URL of the release asset with exactly this name.
fn asset_url<'a>(assets: &'a [serde_json::Value], name: &str) -> Option<&'a str> {
    assets
        .iter()
        .find(|a| a["name"].as_str() == Some(name))
        .and_then(|a| a["browser_download_url"].as_str())
}

/// Look up `file` in `sha256sum` output ("<hex>  <file>", or "<hex> *<file>"
/// in binary mode).
fn expected_checksum<'a>(sums: &'a str, file: &str) -> Option<&'a str> {
    sums.lines().find_map(|line| {
        let (hash, name) = line.split_once(char::is_whitespace)?;
        let name = name.trim_start().trim_start_matches('*');
        (name == file).then_some(hash)
    })
}

/// Is `latest` a newer dotted version than `current`? Non-numeric parts
/// compare as 0, so a pre-release suffix never triggers a downgrade.
fn is_newer(latest: &str, current: &str) -> bool {
    fn parts(v: &str) -> Vec<u64> {
        v.split(['.', '-', '+'])
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    }
    let (mut l, mut c) = (parts(latest), parts(current));
    let len = l.len().max(c.len());
    l.resize(len, 0);
    c.resize(len, 0);
    l > c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_format() {
        let version = env!("CARGO_PKG_VERSION");
        assert!(!version.is_empty());
        // Should be semver-ish
        assert!(version.contains('.'));
    }

    #[test]
    fn test_is_newer() {
        assert!(is_newer("0.4.0", "0.3.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.99.1"));
        assert!(is_newer("0.3.1", "0.3"));
        assert!(!is_newer("0.3.0", "0.3.0"));
        assert!(!is_newer("0.3", "0.3.0"));
        // A local build ahead of the latest release must not be downgraded
        assert!(!is_newer("0.3.0", "0.4.0"));
    }

    #[test]
    fn test_expected_checksum() {
        let sums = "aaa111  zl-x86_64-unknown-linux-gnu\n\
                    bbb222 *zl-x86_64-unknown-linux-musl\n\
                    ccc333  zl-aarch64-unknown-linux-gnu\n";
        assert_eq!(
            expected_checksum(sums, "zl-x86_64-unknown-linux-gnu"),
            Some("aaa111")
        );
        assert_eq!(
            expected_checksum(sums, "zl-x86_64-unknown-linux-musl"),
            Some("bbb222")
        );
        // A missing entry must not fall back to a neighbouring line
        assert_eq!(
            expected_checksum(sums, "zl-aarch64-unknown-linux-musl"),
            None
        );
    }

    #[test]
    fn test_asset_url_requires_exact_name() {
        let assets: Vec<serde_json::Value> = serde_json::from_str(
            r#"[
                {"name": "zl-x86_64-unknown-linux-gnu.sig", "browser_download_url": "sig"},
                {"name": "zl-x86_64-unknown-linux-gnu", "browser_download_url": "bin"},
                {"name": "SHA256SUMS.txt", "browser_download_url": "sums"}
            ]"#,
        )
        .unwrap();
        assert_eq!(
            asset_url(&assets, "zl-x86_64-unknown-linux-gnu"),
            Some("bin")
        );
        assert_eq!(asset_url(&assets, "SHA256SUMS.txt"), Some("sums"));
        assert_eq!(asset_url(&assets, "zl-aarch64-unknown-linux-gnu"), None);
    }

    #[test]
    fn test_release_target_matches_published_assets() {
        // Every target this build can ask for must be one release.yml builds
        let published = [
            "x86_64-unknown-linux-gnu",
            "x86_64-unknown-linux-musl",
            "aarch64-unknown-linux-gnu",
            "aarch64-unknown-linux-musl",
        ];
        if let Some(target) = release_target() {
            assert!(published.contains(&target));
            assert_eq!(target.ends_with("musl"), cfg!(target_env = "musl"));
        }
    }
}
