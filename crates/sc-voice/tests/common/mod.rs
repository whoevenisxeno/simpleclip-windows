use std::path::{Path, PathBuf};
use std::process::Command;

pub fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sc-voice-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

pub fn speak(dir: &Path, items: &[(&str, &[&str])]) -> Option<Vec<PathBuf>> {
    let paths: Vec<PathBuf> = items
        .iter()
        .map(|(name, _)| dir.join(format!("{name}.wav")))
        .collect();
    let list: Vec<String> = items
        .iter()
        .zip(&paths)
        .map(|((_, lines), path)| {
            let lines: Vec<String> = lines.iter().map(|l| quote(l)).collect();
            format!(
                "[void]$items.Add(@({}, @({})))",
                quote(&path.to_string_lossy()),
                lines.join(", ")
            )
        })
        .collect();
    let script = format!(
        r#"$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Speech
$r = [System.Speech.Recognition.SpeechRecognitionEngine]::InstalledRecognizers()
if ($r.Count -eq 0) {{ 'no-recognizer'; exit 0 }}
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer
$v = $s.GetInstalledVoices($r[0].Culture) | Where-Object {{ $_.Enabled }} | Select-Object -First 1
if (-not $v) {{ 'no-voice'; exit 0 }}
$s.SelectVoice($v.VoiceInfo.Name)
$items = New-Object System.Collections.ArrayList
{}
foreach ($item in $items) {{
  $b = New-Object System.Speech.Synthesis.PromptBuilder($s.Voice.Culture)
  $b.AppendText('I am going to play a round now.')
  $b.AppendBreak([TimeSpan]::FromMilliseconds(900))
  foreach ($line in $item[1]) {{ $b.AppendText($line); $b.AppendBreak([TimeSpan]::FromMilliseconds(1200)) }}
  $s.SetOutputToWaveFile($item[0])
  $s.Speak($b)
  $s.SetOutputToNull()
}}
'ok'"#,
        list.join("\n")
    );
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .ok()?;
    let said = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if said != "ok" {
        eprintln!(
            "skipped: text to speech is not usable here ({said}) {}",
            String::from_utf8_lossy(&out.stderr)
        );
        return None;
    }
    for p in &paths {
        assert!(
            p.exists(),
            "text to speech ran but {} is missing",
            p.display()
        );
    }
    Some(paths)
}
