//! Looking for a newer SimpleClip on GitHub. Shared by the app (the "Update
//! available" button) and the daemon (the card shown at start-up). Network access
//! goes through `curl.exe`, which ships with Windows 10 and 11.

use std::process::Command;

const API: &str = "https://api.github.com/repos/whoevenisxeno/simpleclip-windows/releases/latest";

/// A release newer than the running version.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub url: String,
    pub size: u64,
}

/// `4.1.0` / `v4.1.0` as numbers.
fn parse(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches(['v', 'V'])
        .split('.')
        .map(|p| {
            p.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .unwrap_or(0)
        })
        .collect()
}

/// Is `remote` a higher version than `local`?
pub fn is_newer(remote: &str, local: &str) -> bool {
    let (mut r, mut l) = (parse(remote), parse(local));
    let n = r.len().max(l.len()).max(3);
    r.resize(n, 0);
    l.resize(n, 0);
    r > l
}

/// Fetch a URL with curl (no console window on Windows).
pub fn curl(args: &[&str]) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new("curl.exe");
    cmd.args(["-fsSL", "--max-time", "30", "-H", "User-Agent: SimpleClip"])
        .args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd
        .output()
        .map_err(|e| format!("couldn't run curl: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(format!(
            "couldn't reach GitHub ({})",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Read the latest release's JSON: `None` when it isn't newer than `current`.
pub fn parse_release(json: &[u8], current: &str) -> Result<Option<Release>, String> {
    let v: serde_json::Value =
        serde_json::from_slice(json).map_err(|_| "unexpected reply from GitHub".to_string())?;
    let tag = v["tag_name"].as_str().ok_or("no release found")?;
    if !is_newer(tag, current) {
        return Ok(None);
    }
    let asset = v["assets"]
        .as_array()
        .and_then(|a| {
            a.iter().find(|a| {
                a["name"]
                    .as_str()
                    .is_some_and(|n| n.ends_with("-Setup.exe"))
            })
        })
        .ok_or("this release has no installer attached")?;
    Ok(Some(Release {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        notes: v["body"].as_str().unwrap_or("").to_string(),
        url: asset["browser_download_url"]
            .as_str()
            .ok_or("installer has no download link")?
            .to_string(),
        size: asset["size"].as_u64().unwrap_or(0),
    }))
}

/// Ask GitHub for the latest release; `Ok(None)` means `current` is up to date.
pub fn fetch_latest(current: &str) -> Result<Option<Release>, String> {
    curl(&[API]).and_then(|j| parse_release(&j, current))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("v4.1.0", "4.0.0"));
        assert!(is_newer("4.0.1", "4.0.0"));
        assert!(is_newer("5.0.0", "4.9.9"));
        assert!(is_newer("4.10.0", "4.9.0"));
        assert!(!is_newer("4.0.0", "4.0.0"));
        assert!(!is_newer("v3.9.9", "4.0.0"));
        assert!(is_newer("4.0.0.1", "4.0.0"));
    }

    #[test]
    fn release_json_is_read() {
        let json = br###"{"tag_name":"v99.0.0","body":"## 99.0.0\n- x","assets":[
            {"name":"SimpleClip-99.0.0-win64.zip","browser_download_url":"z","size":1},
            {"name":"SimpleClip-99.0.0-Setup.exe","browser_download_url":"https://x/setup.exe","size":42}]}"###;
        let r = parse_release(json, "4.0.0").unwrap().unwrap();
        assert_eq!(r.version, "99.0.0");
        assert_eq!(r.url, "https://x/setup.exe");
        assert_eq!(r.size, 42);
        let old = br#"{"tag_name":"v0.0.1","assets":[]}"#;
        assert!(parse_release(old, "4.0.0").unwrap().is_none());
    }
}
