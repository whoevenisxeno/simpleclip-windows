mod client;

use anyhow::Result;
use clap::{Parser, Subcommand};
use sc_core::ipc::{Request, Response};

#[derive(Parser)]
#[command(name = "sc", about = "SimpleClip control client")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Save {
        #[arg(long)]
        last: Option<u32>,
    },
    Screenshot,
    Record,
    Stop,
    Status {
        #[arg(long)]
        json: bool,
    },
    Pause,
    Resume,
    Quit,
    Thumbnail {
        path: std::path::PathBuf,
        #[arg(long, default_value_t = 480)]
        width: u32,
        #[arg(long)]
        at: Option<f64>,
    },
    Trim {
        path: std::path::PathBuf,
        #[arg(long, default_value_t = 0.0)]
        start: f64,
        #[arg(long)]
        end: f64,
    },
    Compress {
        path: std::path::PathBuf,
        #[arg(long, default_value_t = 10)]
        mb: u32,
    },
    Monitors,
    AudioDevices,
    TestNotification,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let (req, json) = match cli.command {
        Command::Save { last } => (Request::Save { last_secs: last }, false),
        Command::Screenshot => (Request::Screenshot, false),
        Command::Record => (Request::Record, false),
        Command::Stop => (Request::Stop, false),
        Command::Status { json } => (Request::Status, json),
        Command::Pause => (Request::Pause, false),
        Command::Resume => (Request::Resume, false),
        Command::Quit => (Request::Shutdown, false),
        Command::Thumbnail { path, width, at } => {
            let path = std::path::absolute(&path).unwrap_or(path);
            let req = match at {
                Some(secs) => Request::Frame { path, secs, width },
                None => Request::Thumbnail { path, width },
            };
            (req, false)
        }
        Command::Trim { path, start, end } => (
            Request::Trim {
                path: std::path::absolute(&path).unwrap_or(path),
                start_secs: start,
                end_secs: end,
                dest: Default::default(),
            },
            false,
        ),
        Command::Compress { path, mb } => (
            Request::Compress {
                path: std::path::absolute(&path).unwrap_or(path),
                target_mb: mb,
            },
            false,
        ),
        Command::Monitors => (Request::ListMonitors, false),
        Command::AudioDevices => (Request::ListAudioDevices, false),
        Command::TestNotification => (Request::TestNotification, false),
    };

    let resp = client::request(req)?;
    match resp {
        Response::Status(s) if json => println!("{}", serde_json::to_string_pretty(&s)?),
        Response::Status(s) => print_status(&s),
        Response::Saved {
            path,
            duration_secs,
        } => {
            println!("saved {} ({:.1}s)", path.display(), duration_secs)
        }
        Response::Monitors { monitors: m } => {
            for mon in m {
                println!(
                    "{:<3} {} ({}x{}{})",
                    mon.id,
                    mon.name,
                    mon.width,
                    mon.height,
                    if mon.primary { ", primary" } else { "" }
                );
            }
        }
        Response::AudioDevices { devices: d } => {
            for dev in d {
                println!("{}  {}", dev.id, dev.name);
            }
        }
        Response::Probed { duration_secs } => println!("{duration_secs:.2}s"),
        Response::Thumbnail { path } => println!("{}", path.display()),
        Response::Ok | Response::Pong => println!("ok"),
        Response::Error { message } => {
            eprintln!("error: {message}");
            std::process::exit(1);
        }
    }
    Ok(())
}

fn print_status(s: &sc_core::ipc::StatusReport) {
    println!("state:     {:?}", s.state);
    println!("recording: {}", s.recording);
    println!(
        "buffer:    {}s ({:.0}% full)",
        s.buffer_secs,
        s.buffer_fill * 100.0
    );
    match &s.monitor {
        Some(m) => println!("monitor:   {} ({}x{})", m.name, m.width, m.height),
        None => println!("monitor:   none"),
    }
    match s.encoder {
        Some(e) => println!("encoder:   {:?}", e),
        None => println!("encoder:   none"),
    }
    println!("drift:     {:.1} ms", s.drift_ms);
    println!("daemon:    v{}", s.daemon_version);
}
