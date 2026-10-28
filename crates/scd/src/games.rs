use std::path::Path;

const LIBRARIES: &[&str] = &[
    "\\steamapps\\common\\",
    "\\epic games\\",
    "\\gog galaxy\\games\\",
    "\\gog games\\",
    "\\riot games\\",
    "\\ubisoft game launcher\\games\\",
    "\\xboxgames\\",
    "\\ea games\\",
    "\\origin games\\",
    "\\rockstar games\\",
];

const NOT_GAMES: &[&str] = &[
    "launcher",
    "riot client",
    "epic online services",
    "directxredist",
    "steamworks shared",
    "steam controller configs",
    "easyanticheat",
    "battleye",
];

const KNOWN: &[(&str, &str)] = &[
    ("valorant-win64-shipping", "VALORANT"),
    ("league of legends", "League of Legends"),
    ("fortniteclient-win64-shipping", "Fortnite"),
    ("minecraft.windows", "Minecraft"),
    ("robloxplayerbeta", "Roblox"),
    ("cs2", "Counter-Strike 2"),
    ("csgo", "Counter-Strike"),
    ("dota2", "Dota 2"),
    ("overwatch", "Overwatch 2"),
    ("r5apex", "Apex Legends"),
    ("r5apex_dx12", "Apex Legends"),
    ("rainbowsix", "Rainbow Six Siege"),
    ("rainbowsix_vulkan", "Rainbow Six Siege"),
    ("rainbowsix_be", "Rainbow Six Siege"),
    ("gta5", "Grand Theft Auto V"),
    ("rdr2", "Red Dead Redemption 2"),
    ("rocketleague", "Rocket League"),
    ("eldenring", "Elden Ring"),
    ("cyberpunk2077", "Cyberpunk 2077"),
    ("witcher3", "The Witcher 3"),
    ("wow", "World of Warcraft"),
    ("destiny2", "Destiny 2"),
    ("tslgame", "PUBG"),
    ("cod", "Call of Duty"),
    ("modernwarfare", "Call of Duty"),
    ("hogwartslegacy", "Hogwarts Legacy"),
    ("starfield", "Starfield"),
    ("terraria", "Terraria"),
    ("stardew valley", "Stardew Valley"),
    ("factorio", "Factorio"),
    ("among us", "Among Us"),
    ("helldivers2", "Helldivers 2"),
    ("palworld-win64-shipping", "Palworld"),
    ("bg3", "Baldur's Gate 3"),
    ("bg3_dx11", "Baldur's Gate 3"),
    ("deadlock", "Deadlock"),
    ("marvel-win64-shipping", "Marvel Rivals"),
];

const NEVER: &[&str] = &[
    "chrome",
    "msedge",
    "firefox",
    "waterfox",
    "brave",
    "opera",
    "vivaldi",
    "arc",
    "iexplore",
    "explorer",
    "scd",
    "sc-gui",
    "discord",
    "spotify",
    "code",
    "devenv",
    "obs64",
    "obs32",
    "vlc",
    "mpv",
    "mpc-hc64",
    "wmplayer",
    "video.ui",
    "photos",
    "powerpnt",
    "acrord32",
    "teams",
    "zoom",
    "slack",
    "steam",
    "steamwebhelper",
    "epicgameslauncher",
    "battle.net",
    "galaxyclient",
    "ubisoftconnect",
    "upc",
    "windowsterminal",
    "cmd",
    "powershell",
];

pub fn detect(exe: &Path, fullscreen: bool) -> Option<String> {
    let stem = exe.file_stem()?.to_str()?.to_ascii_lowercase();
    if NEVER.contains(&stem.as_str()) {
        return None;
    }
    if let Some(name) = library_name(exe) {
        return Some(name);
    }
    if let Some((_, name)) = KNOWN.iter().find(|(s, _)| *s == stem) {
        return Some((*name).to_string());
    }
    if fullscreen {
        let name = tidy(&stem);
        return (!name.is_empty()).then_some(name);
    }
    None
}

fn library_name(exe: &Path) -> Option<String> {
    let full = exe.to_str()?.replace('/', "\\");
    let lower = full.to_ascii_lowercase();
    for lib in LIBRARIES {
        let Some(at) = lower.find(lib) else { continue };
        let rest = &full[at + lib.len()..];
        let folder = rest.split('\\').next()?.trim();
        if folder.is_empty() || !rest.contains('\\') {
            continue;
        }
        if NOT_GAMES.contains(&folder.to_ascii_lowercase().as_str()) {
            return None;
        }
        return Some(folder.to_string());
    }
    None
}

fn tidy(stem: &str) -> String {
    let mut s = stem.to_string();
    for suffix in [
        "-win64-shipping",
        "-win64",
        "_x64",
        "_dx12",
        "_dx11",
        "_vulkan",
        "-shipping",
    ] {
        if let Some(t) = s.strip_suffix(suffix) {
            s = t.to_string();
        }
    }
    s.split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn steam_games_use_their_folder_name() {
        let exe = p(
            r"C:\Program Files (x86)\Steam\steamapps\common\Counter-Strike Global Offensive\game\bin\win64\cs2.exe",
        );
        assert_eq!(
            detect(&exe, false).as_deref(),
            Some("Counter-Strike Global Offensive")
        );
    }

    #[test]
    fn other_stores_work_and_their_launchers_do_not() {
        assert_eq!(
            detect(
                &p(r"D:\Epic Games\Fortnite\FortniteGame\Binaries\Win64\x.exe"),
                false
            )
            .as_deref(),
            Some("Fortnite")
        );
        assert_eq!(
            detect(&p(r"C:\Riot Games\VALORANT\live\VALORANT.exe"), false).as_deref(),
            Some("VALORANT")
        );
        assert!(detect(
            &p(r"C:\Program Files\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe"),
            false
        )
        .is_none());
        assert!(detect(
            &p(r"C:\Riot Games\Riot Client\RiotClientServices.exe"),
            false
        )
        .is_none());
        assert!(detect(&p(r"C:\Epic Games\setup.exe"), false).is_none());
    }

    #[test]
    fn well_known_games_are_recognised_anywhere() {
        assert_eq!(
            detect(&p(r"E:\Stuff\RainbowSix.exe"), false).as_deref(),
            Some("Rainbow Six Siege")
        );
        assert_eq!(
            detect(&p(r"C:\Games\GTA5.exe"), false).as_deref(),
            Some("Grand Theft Auto V")
        );
    }

    #[test]
    fn everyday_programs_are_never_games() {
        for exe in [
            r"C:\Program Files\Mozilla Firefox\firefox.exe",
            r"C:\Windows\explorer.exe",
            r"C:\Users\me\AppData\Local\Discord\app\Discord.exe",
            r"C:\Program Files\Microsoft VS Code\Code.exe",
            r"C:\Program Files\Steam\steam.exe",
        ] {
            assert!(detect(&p(exe), false).is_none(), "{exe}");
            assert!(detect(&p(exe), true).is_none(), "{exe} full screen");
        }
    }

    #[test]
    fn unknown_programs_need_exclusive_full_screen() {
        let exe = p(r"C:\Stuff\SomeGame-Win64-Shipping.exe");
        assert!(detect(&exe, false).is_none());
        assert_eq!(detect(&exe, true).as_deref(), Some("Somegame"));
        assert_eq!(tidy("my_cool-game_x64"), "My Cool Game");
    }
}
