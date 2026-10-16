//! CLI entry point. Clap handles help and syntax errors before command execution.
//! Command modules own execution; daemon owns the long-running lifecycle.
//! Runtime failures return through main.rs, which prints them and exits with 1.

use clap::Parser;

mod capture;
mod cli;
mod commands;
mod daemon;
mod presentation;
mod profile;
mod replay;

use capture::dump;
use cli::{Cli, Command};
use commands::{devices, doctor, proxy, status};
use daemon::daemon;
use replay::{replay, replay_raw};

pub(super) fn run() -> Result<i32, String> {
    match Cli::parse().command {
        Command::Replay(args) => replay(&args).map(|()| 0),
        Command::ReplayRaw(args) => replay_raw(&args).map(|()| 0),
        Command::Devices(args) => devices(&args.root, args.show_all).map(|()| 0),
        Command::Doctor(args) => doctor(&args.into()),
        Command::Status(args) => status(&args.into()),
        Command::Dump(args) => {
            let device_path = args
                .device
                .resolve(&args.input_root)
                .map_err(|err| err.to_string())?;
            dump(&device_path, &args.out, args.frames, args.raw).map(|()| 0)
        }
        Command::Proxy(args) => proxy(&args).map(|()| 0),
        Command::Daemon(args) => daemon(&args.load()?).map(|()| 0),
    }
}
