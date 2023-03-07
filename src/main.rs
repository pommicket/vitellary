#![warn(clippy::pedantic)]
#![allow(clippy::assertions_on_constants, clippy::uninlined_format_args)]

mod game;

use crate::game::{Game, Revision, Update};
use anyhow::anyhow;
use anyhow::{bail, Context, Result};
use argh::FromArgs;
use env_logger::Env;
use game::Event;
use read_process_memory::Pid;
use std::io::BufRead;
use std::net::{SocketAddr, TcpListener};
use std::process::Command;
use std::time::Duration;
use tungstenite::Message;

#[allow(clippy::doc_markdown)] // lol
#[derive(FromArgs)]
/// Attach to a VVVVVV process and provide a LiveSplit One server.
struct Args {
    /// enable verbose logging output
    #[argh(switch, short = 'v')]
    verbose: bool,

    /// bind address for WebSocket (default: 127.0.0.1:5555)
    #[argh(option)]
    bind: Option<SocketAddr>,

    /// which revision of VVVVVV you have
    ///
    /// this can be a version number (e.g. "2.3") or a commit ID (e.g. "48cddf57a67a90be0b6f6d8a780f766ca15942a7").
    #[argh(option, default = "String::from(\"master\")")]
    revision: String,

    /// process ID of a specific VVVVVV process
    #[argh(positional)]
    pid: Option<Pid>,
}

fn main() -> Result<()> {
    let args: Args = argh::from_env();
    env_logger::Builder::from_env(Env::default().default_filter_or(if args.verbose {
        "vitellary=debug"
    } else {
        "vitellary=info"
    }))
    .init();

    let revision = Revision::get(&args.revision).ok_or_else(|| anyhow!("no such revision"))?;

    let pid = if let Some(pid) = args.pid {
        pid
    } else {
        let output = Command::new("pgrep")
            .args(["-n", "VVVVVV"])
            .output()
            .context("failed to run pgrep")?;
        if output.status.success() {
            output
                .stdout
                .lines()
                .next()
                .expect("pgrep returned 0 with no output")
                .expect("pgrep output invalid UTF-8")
                .parse()?
        } else if output.status.code() == Some(1) {
            bail!("no VVVVVV process found");
        } else {
            bail!("pgrep failed with {}", output.status);
        }
    };

    let mut game = Game::attach(pid)?;
    let (sender, receiver) = crossbeam_channel::bounded::<Update>(10);

    let bind = args.bind.unwrap_or_else(|| ([127, 0, 0, 1], 5555).into());
    let server = TcpListener::bind(bind).context("failed to bind WebSocket address")?;
    log::info!("listening on ws://{}", bind);
    std::thread::spawn(move || {
        let receiver = receiver;
        for stream in server.incoming() {
            let receiver = receiver.clone();
            std::thread::spawn(move || -> Result<()> {
                let mut websocket = tungstenite::accept(stream.unwrap())?;
                loop {
                    let update = receiver.recv()?;
                    websocket.write_message(Message::Text(format!(
                        "setgametime {}.{:02}",
                        update.time.as_secs(),
                        update.time.subsec_nanos() / 10_000_000
                    )))?;
                    if let Some(event) = update.event {
                        websocket.write_message(Message::Text(
                            match event {
                                Event::NewGame => "start",
                                Event::Verdigris
                                | Event::Vermilion
                                | Event::Victoria
                                | Event::Violet
                                | Event::Vitellary
                                | Event::IntermissionOne
                                | Event::IntermissionTwo
                                | Event::GameComplete => "split",
                                Event::Reset => "reset",
                            }
                            .into(),
                        ))?;
                    }
                }
            });
        }
    });

    loop {
        sender.try_send(game.update(revision)?).ok();
        std::thread::sleep(Duration::from_millis(10));
    }
}
