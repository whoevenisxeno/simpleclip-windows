use std::path::PathBuf;

/// Keep a SimpleClip-owned Hyprland snippet in sync with the configured save
/// hotkey, so the hotkey is set entirely from the app with no elevated
/// permissions. Writes `~/.config/hypr/simpleclip.conf`, sources it once from
/// the main config, and reloads Hyprland only when the bind actually changed.
pub fn sync_hotkey(save_hotkey: &str) {
    let Some(bind) = to_bind(save_hotkey) else {
        tracing::warn!(
            hotkey = save_hotkey,
            "unparseable hotkey; leaving compositor bind alone"
        );
        return;
    };
    let sc = sc_binary();
    let content = format!(
        "# Managed by SimpleClip. Set the hotkey in the app, not here.\n\
         bind = {bind}, exec, {sc} save\n"
    );

    let Some(conf) = hypr_dir().map(|d| d.join("simpleclip.conf")) else {
        return;
    };
    if std::fs::read_to_string(&conf).ok().as_deref() == Some(content.as_str()) {
        return; // already up to date; don't reload the compositor needlessly
    }
    if let Some(parent) = conf.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::write(&conf, &content).is_err() {
        return;
    }
    ensure_sourced();
    reload();
    tracing::info!(hotkey = save_hotkey, "compositor save-hotkey synced");
}

fn hypr_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.config_dir().join("hypr"))
}

fn sc_binary() -> String {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().join(".local/bin/sc"))
        .filter(|p| p.exists())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "sc".into())
}

/// Convert "SUPER+F10" into Hyprland's "SUPER, F10" bind prefix.
fn to_bind(spec: &str) -> Option<String> {
    let parts: Vec<&str> = spec
        .split('+')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    let (key, mods) = parts.split_last()?;
    let mut mod_names = Vec::new();
    for m in mods {
        let name = match m.to_uppercase().as_str() {
            "SUPER" | "META" | "WIN" | "MOD" => "SUPER",
            "CTRL" | "CONTROL" => "CTRL",
            "ALT" => "ALT",
            "SHIFT" => "SHIFT",
            _ => return None,
        };
        mod_names.push(name);
    }
    Some(format!("{}, {}", mod_names.join(" "), key.to_uppercase()))
}

fn ensure_sourced() {
    let Some(dir) = hypr_dir() else { return };
    let main = dir.join("hyprland.conf");
    let Ok(text) = std::fs::read_to_string(&main) else {
        return;
    };
    if text.contains("simpleclip.conf") {
        return;
    }
    let line = "\n# SimpleClip hotkey (managed)\nsource = ~/.config/hypr/simpleclip.conf\n";
    let _ = std::fs::write(&main, format!("{text}{line}"));
}

fn reload() {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
        return; // not a running Hyprland session
    }
    let _ = std::process::Command::new("hyprctl").arg("reload").spawn();
}
