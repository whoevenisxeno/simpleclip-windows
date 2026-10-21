//! Discord Rich Presence: shows the game you are in and how long you have been
//! clipping on your Discord profile.
//!
//! It talks to the Discord app running on this PC through its local named pipe
//! (`\\.\pipe\discord-ipc-N`), the same way games do. Nothing is sent from here
//! over the network; the Discord app shows what it is told. If Discord is not
//! running the thread just keeps checking quietly.

use crate::state::Daemon;
use sc_core::capture::CaptureState;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The Discord application this presence belongs to (its name is what Discord
/// shows after "Playing").
const APP_ID: &str = "1562536909262164019";

/// Discord drops presence updates that come faster than this.
const MIN_GAP: Duration = Duration::from_secs(15);
/// How often to look for a change, or for Discord to start.
const POLL: Duration = Duration::from_secs(5);

const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
const OP_CLOSE: u32 = 2;

/// One packet on the pipe: opcode, length, then JSON.
fn encode(op: u32, json: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + json.len());
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(json.as_bytes());
    out
}

/// What the profile shows. Two different values mean a new update is due.
#[derive(Clone, PartialEq, Debug)]
struct Presence {
    game: Option<String>,
    paused: bool,
    /// Unix seconds when clipping began, for Discord's "elapsed" counter.
    started: u64,
}

/// The `SET_ACTIVITY` message for a presence (or clearing it with `None`).
fn activity_message(presence: Option<&Presence>, pid: u32, nonce: u64) -> String {
    let activity = presence.map(|p| {
        serde_json::json!({
            "details": match &p.game {
                Some(g) => format!("Playing {g}"),
                None => "On the desktop".to_string(),
            },
            "state": if p.paused { "Replay buffer paused" } else { "Clipping with SimpleClip" },
            "timestamps": { "start": p.started },
        })
    });
    serde_json::json!({
        "cmd": "SET_ACTIVITY",
        "args": { "pid": pid, "activity": activity },
        "nonce": nonce.to_string(),
    })
    .to_string()
}

/// An open connection to the Discord app.
struct Conn {
    pipe: File,
    nonce: u64,
}

impl Conn {
    /// Find the running Discord app and say hello. Discord may be on any of ten pipes.
    fn open() -> Option<Self> {
        for n in 0..10 {
            let path = format!(r"\\.\pipe\discord-ipc-{n}");
            let Ok(pipe) = OpenOptions::new().read(true).write(true).open(path) else {
                continue;
            };
            let mut conn = Conn { pipe, nonce: 0 };
            let hello = serde_json::json!({ "v": 1, "client_id": APP_ID }).to_string();
            // Discord answers with a READY event; anything else (a bad app id, a
            // close) means this is not a usable connection.
            match conn.send(OP_HANDSHAKE, &hello) {
                Ok(reply) if reply.contains("READY") => return Some(conn),
                Ok(reply) => tracing::warn!(%reply, "discord refused the handshake"),
                Err(_) => {}
            }
        }
        None
    }

    /// Write a packet and read Discord's reply. Any failure means the connection is
    /// dead (Discord closed or restarted) and the caller should drop it.
    fn send(&mut self, op: u32, json: &str) -> std::io::Result<String> {
        self.pipe.write_all(&encode(op, json))?;
        let mut header = [0u8; 8];
        self.pipe.read_exact(&mut header)?;
        let reply_op = u32::from_le_bytes(header[0..4].try_into().unwrap());
        let len = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        let mut body = vec![0u8; len.min(1 << 20)];
        self.pipe.read_exact(&mut body)?;
        if reply_op == OP_CLOSE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "Discord closed the connection",
            ));
        }
        let reply = String::from_utf8_lossy(&body).into_owned();
        tracing::debug!(%reply, "discord reply");
        Ok(reply)
    }

    fn set(&mut self, presence: Option<&Presence>) -> std::io::Result<String> {
        self.nonce += 1;
        let msg = activity_message(presence, std::process::id(), self.nonce);
        self.send(OP_FRAME, &msg)
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// "valorant" becomes "Valorant", matching how the Clips page names a game.
fn game_name(app: &str) -> String {
    let mut c = app.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Keep the Discord profile in step with what SimpleClip is doing, until the
/// process ends.
pub fn spawn(daemon: Daemon) {
    std::thread::Builder::new()
        .name("sc-discord".into())
        .spawn(move || run(daemon))
        .expect("discord thread");
}

fn run(daemon: Daemon) {
    let mut conn: Option<Conn> = None;
    let mut started: Option<u64> = None;
    let mut game: Option<String> = None;
    let mut shown: Option<Presence> = None;
    let mut last_sent = Instant::now() - MIN_GAP;
    let mut warned = false;
    loop {
        std::thread::sleep(POLL);
        let enabled = daemon.config.read().unwrap().general.discord_presence;
        let state = daemon.capture_state();
        let clipping = matches!(state, CaptureState::Active | CaptureState::Paused);
        if !clipping {
            started = None; // the next time it runs, the timer starts again
        }

        if !enabled || !clipping {
            // Take the presence down once, then stop talking to Discord.
            if let Some(mut c) = conn.take() {
                let _ = c.set(None);
            }
            shown = None;
            continue;
        }

        // The game is whatever you are in; the desktop and our own windows
        // leave the last one showing.
        if let Some(app) = crate::naming::foreground_app() {
            game = Some(game_name(&app));
        }
        let want = Presence {
            game: game.clone(),
            paused: state == CaptureState::Paused,
            started: *started.get_or_insert_with(unix_now),
        };

        if conn.is_none() {
            conn = Conn::open();
            if conn.is_some() {
                tracing::info!("discord rich presence connected");
                warned = false;
                shown = None;
            } else if !warned {
                tracing::debug!("discord is not running; rich presence waits");
                warned = true;
            }
        }
        let Some(c) = conn.as_mut() else { continue };
        if shown.as_ref() == Some(&want) || last_sent.elapsed() < MIN_GAP {
            continue;
        }
        match c.set(Some(&want)) {
            Ok(reply) => {
                if reply.contains("\"ERROR\"") {
                    tracing::warn!(%reply, "discord rejected the presence");
                }
                shown = Some(want);
                last_sent = Instant::now();
            }
            Err(e) => {
                tracing::info!(error = %e, "discord connection lost");
                conn = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packets_carry_opcode_and_length() {
        let p = encode(1, "{}");
        assert_eq!(&p[0..4], &1u32.to_le_bytes());
        assert_eq!(&p[4..8], &2u32.to_le_bytes());
        assert_eq!(&p[8..], b"{}");
    }

    #[test]
    fn presence_shows_game_and_start_time() {
        let p = Presence {
            game: Some("Valorant".into()),
            paused: false,
            started: 1_700_000_000,
        };
        let v: serde_json::Value =
            serde_json::from_str(&activity_message(Some(&p), 42, 7)).unwrap();
        assert_eq!(v["cmd"], "SET_ACTIVITY");
        assert_eq!(v["args"]["pid"], 42);
        assert_eq!(v["nonce"], "7");
        assert_eq!(v["args"]["activity"]["details"], "Playing Valorant");
        assert_eq!(v["args"]["activity"]["state"], "Clipping with SimpleClip");
        assert_eq!(
            v["args"]["activity"]["timestamps"]["start"],
            1_700_000_000u64
        );
    }

    #[test]
    fn desktop_and_paused_read_sensibly() {
        let p = Presence {
            game: None,
            paused: true,
            started: 1,
        };
        let v: serde_json::Value = serde_json::from_str(&activity_message(Some(&p), 1, 1)).unwrap();
        assert_eq!(v["args"]["activity"]["details"], "On the desktop");
        assert_eq!(v["args"]["activity"]["state"], "Replay buffer paused");
    }

    #[test]
    fn clearing_sends_a_null_activity() {
        let v: serde_json::Value = serde_json::from_str(&activity_message(None, 1, 1)).unwrap();
        assert!(v["args"]["activity"].is_null());
    }

    #[test]
    fn game_names_start_with_a_capital() {
        assert_eq!(game_name("valorant"), "Valorant");
        assert_eq!(game_name(""), "");
    }

    /// Connects and says hello only; nothing shows on the profile.
    #[test]
    #[ignore = "talks to the running Discord app"]
    fn discord_accepts_the_app_id() {
        assert!(
            Conn::open().is_some(),
            "Discord not running or refused the app id"
        );
    }

    /// Run with `cargo test -p scd -- --ignored discord` (SC_PRESENCE_TEST_SECS sets how long it stays up, 4 by default): shows the presence on the
    /// real Discord profile for a few seconds, then clears it.
    #[test]
    #[ignore = "talks to the running Discord app"]
    fn discord_accepts_the_presence() {
        let mut conn = Conn::open().expect("Discord is running and accepted the app id");
        let p = Presence {
            game: Some("Test game".into()),
            paused: false,
            started: unix_now(),
        };
        let reply = conn.set(Some(&p)).expect("presence sent");
        assert!(!reply.contains("\"ERROR\""), "Discord said: {reply}");
        println!("Discord replied: {reply}");
        let secs = std::env::var("SC_PRESENCE_TEST_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(4);
        std::thread::sleep(Duration::from_secs(secs));
        conn.set(None).expect("presence cleared");
    }
}
