use std::process::Command;

const DOWNLOAD_PREFIX: &str =
    "https://github.com/whoevenisxeno/simpleclip-windows/releases/download/";
const API: &str = "https://api.github.com/repos/whoevenisxeno/simpleclip-windows/releases/latest";

#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub url: String,
    pub size: u64,
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub app: Option<Asset>,
    pub full: Option<Asset>,
    pub dlls: Option<Asset>,
}

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

pub fn is_newer(remote: &str, local: &str) -> bool {
    let (mut r, mut l) = (parse(remote), parse(local));
    let n = r.len().max(l.len()).max(3);
    r.resize(n, 0);
    l.resize(n, 0);
    r > l
}

pub fn major_gap(remote: &str, local: &str) -> u64 {
    let first = |v: &str| parse(v).first().copied().unwrap_or(0);
    first(remote).saturating_sub(first(local))
}

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

fn find_asset(assets: &[serde_json::Value], suffix: &str) -> Result<Option<Asset>, String> {
    let Some(a) = assets
        .iter()
        .find(|a| a["name"].as_str().is_some_and(|n| n.ends_with(suffix)))
    else {
        return Ok(None);
    };
    let url = a["browser_download_url"]
        .as_str()
        .ok_or("an update file has no download link")?;
    if !url.starts_with(DOWNLOAD_PREFIX) {
        return Err("an update file is not hosted where SimpleClip expects".to_string());
    }
    Ok(Some(Asset {
        url: url.to_string(),
        size: a["size"].as_u64().unwrap_or(0),
        sha256: a["digest"]
            .as_str()
            .and_then(|d| d.strip_prefix("sha256:"))
            .map(|h| h.to_ascii_lowercase()),
    }))
}

#[derive(Clone, Debug, PartialEq)]
pub struct AddonPackage {
    pub version: String,
    pub asset: Asset,
}

pub fn parse_addon(json: &[u8], suffix: &str) -> Result<Option<AddonPackage>, String> {
    let v: serde_json::Value =
        serde_json::from_slice(json).map_err(|_| "unexpected reply from GitHub".to_string())?;
    let tag = v["tag_name"].as_str().ok_or("no release found")?;
    let assets = v["assets"].as_array().cloned().unwrap_or_default();
    Ok(find_asset(&assets, suffix)?.map(|asset| AddonPackage {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        asset,
    }))
}

pub fn fetch_addon(suffix: &str) -> Result<Option<AddonPackage>, String> {
    curl(&[API]).and_then(|j| parse_addon(&j, suffix))
}

pub fn parse_release(json: &[u8], current: &str) -> Result<Option<Release>, String> {
    let v: serde_json::Value =
        serde_json::from_slice(json).map_err(|_| "unexpected reply from GitHub".to_string())?;
    let tag = v["tag_name"].as_str().ok_or("no release found")?;
    if !is_newer(tag, current) {
        return Ok(None);
    }
    let assets = v["assets"].as_array().cloned().unwrap_or_default();
    let find = |suffix: &str| find_asset(&assets, suffix);
    let (app, full, dlls) = (
        find("-app.zip")?,
        find("-win64.zip")?,
        find("-dlls.sha256")?,
    );
    if app.is_none() && full.is_none() {
        return Err("this release has no update package attached".into());
    }
    Ok(Some(Release {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        notes: v["body"].as_str().unwrap_or("").to_string(),
        app,
        full,
        dlls,
    }))
}

pub fn fetch_latest(current: &str) -> Result<Option<Release>, String> {
    curl(&[API]).and_then(|j| parse_release(&j, current))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_addon_package_is_found_by_its_suffix() {
        let base = DOWNLOAD_PREFIX;
        let json = format!(
            r###"{{"tag_name":"v7.0.0","assets":[
            {{"name":"SimpleClip-7.0.0-app.zip","browser_download_url":"{base}v7.0.0/SimpleClip-7.0.0-app.zip","size":1}},
            {{"name":"SimpleClip-7.0.0-events.zip","browser_download_url":"{base}v7.0.0/SimpleClip-7.0.0-events.zip","size":77,"digest":"sha256:ABC"}}]}}"###
        );
        let pkg = parse_addon(json.as_bytes(), "-events.zip")
            .unwrap()
            .unwrap();
        assert_eq!(pkg.version, "7.0.0");
        assert_eq!(pkg.asset.size, 77);
        assert_eq!(pkg.asset.sha256.as_deref(), Some("abc"));
        assert!(pkg.asset.url.ends_with("-events.zip"));
    }

    #[test]
    fn a_release_without_the_addon_has_none() {
        let json = br###"{"tag_name":"v7.0.0","assets":[]}"###;
        assert_eq!(parse_addon(json, "-events.zip").unwrap(), None);
    }

    #[test]
    fn an_addon_hosted_elsewhere_is_refused() {
        let json = br###"{"tag_name":"v7.0.0","assets":[{"name":"x-events.zip","browser_download_url":"https://evil.example/x-events.zip","size":1}]}"###;
        assert!(parse_addon(json, "-events.zip").is_err());
    }

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
    fn major_gap_counts_major_versions() {
        assert_eq!(major_gap("v9.1.0", "4.7.2"), 5);
        assert_eq!(major_gap("4.9.9", "4.0.0"), 0);
        assert_eq!(major_gap("3.0.0", "4.0.0"), 0);
    }

    #[test]
    fn release_json_is_read() {
        let base = "https://github.com/whoevenisxeno/simpleclip-windows/releases/download/v99.0.0/";
        let json = format!(
            r###"{{"tag_name":"v99.0.0","body":"## 99.0.0\n- x","assets":[
            {{"name":"SimpleClip-99.0.0-Setup.exe","browser_download_url":"{base}SimpleClip-99.0.0-Setup.exe","size":9}},
            {{"name":"SimpleClip-99.0.0-app.zip","browser_download_url":"{base}SimpleClip-99.0.0-app.zip","size":42,"digest":"sha256:ABCDEF"}},
            {{"name":"SimpleClip-99.0.0-win64.zip","browser_download_url":"{base}SimpleClip-99.0.0-win64.zip","size":99}},
            {{"name":"SimpleClip-99.0.0-dlls.sha256","browser_download_url":"{base}SimpleClip-99.0.0-dlls.sha256","size":5}}]}}"###
        );
        let r = parse_release(json.as_bytes(), "4.0.0").unwrap().unwrap();
        assert_eq!(r.version, "99.0.0");
        let app = r.app.unwrap();
        assert!(app.url.ends_with("SimpleClip-99.0.0-app.zip"));
        assert_eq!(app.sha256.as_deref(), Some("abcdef"));
        assert_eq!(app.size, 42);
        assert_eq!(r.full.unwrap().size, 99);
        assert!(r.dlls.is_some());
        let evil = br###"{"tag_name":"v99.0.0","assets":[{"name":"x-app.zip","browser_download_url":"https://evil.example/x-app.zip","size":1}]}"###;
        assert!(parse_release(evil, "4.0.0").is_err());
        let none = br###"{"tag_name":"v99.0.0","assets":[{"name":"x-Setup.exe","browser_download_url":"https://github.com/whoevenisxeno/simpleclip-windows/releases/download/v99.0.0/x-Setup.exe","size":1}]}"###;
        assert!(parse_release(none, "4.0.0").is_err());
        let old = br#"{"tag_name":"v0.0.1","assets":[]}"#;
        assert!(parse_release(old, "4.0.0").unwrap().is_none());
    }
}
