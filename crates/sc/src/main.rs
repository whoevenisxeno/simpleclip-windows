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
    /// Save the last N seconds from the replay buffer.
    Save {
        #[arg(long)]
        last: Option<u32>,
    },
    /// Save a still frame from the live capture.
    Screenshot,
    /// Start a manual recording.
    Record,
    /// Stop the manual recording.
    Stop,
    /// Show capture state, buffer fill, encoder and drift.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Pause capture.
    Pause,
    /// Resume capture.
    Resume,
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
        Response::Monitors(m) => println!("{}", serde_json::to_string_pretty(&m)?),
        Response::AudioDevices(d) => println!("{}", serde_json::to_string_pretty(&d)?),
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
