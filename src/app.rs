//! CLI entry point: help/version short-circuit before parsing or device access.
//! Parsers own argument validation; command modules own execution; daemon owns
//! the long-running lifecycle. main.rs only presents errors and exits.

use std::env;

mod capture;
mod cli;
mod commands;
mod daemon;
mod help;
mod presentation;
mod profile;
mod replay;

use capture::dump;
use cli::{
    parse_daemon_args, parse_devices_args, parse_doctor_args, parse_dump_args, parse_proxy_args,
    parse_replay_args, parse_status_args,
};
use commands::{devices, doctor, proxy, status};
use daemon::daemon;
use help::*;
use replay::{replay, replay_raw};

pub(super) fn run() -> Result<i32, String> {
    let mut args = env::args().skip(1);

    match args.next().as_deref() {
        Some("--help") | Some("-h") => {
            print_help(HELP);
            Ok(0)
        }
        Some("--version") | Some("-V") => {
            println!("edgepad {}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        Some("replay") => {
            let args = args.collect::<Vec<_>>();
            if is_help_request(&args) {
                print_help(REPLAY_HELP);
                return Ok(0);
            }
            let args = parse_replay_args(args.into_iter(), REPLAY_USAGE, "replay")?;
            replay(&args).map(|()| 0)
        }
        Some("replay-raw") => {
            let args = args.collect::<Vec<_>>();
            if is_help_request(&args) {
                print_help(REPLAY_RAW_HELP);
                return Ok(0);
            }
            let args = parse_replay_args(args.into_iter(), REPLAY_RAW_USAGE, "replay-raw")?;
            replay_raw(&args).map(|()| 0)
        }
        Some("devices") => {
            let args = args.collect::<Vec<_>>();
            if is_help_request(&args) {
                print_help(DEVICES_HELP);
                return Ok(0);
            }
            let args = parse_devices_args(args.into_iter())?;
            devices(&args.root, args.show_all).map(|()| 0)
        }
        Some("doctor") => {
            let args = args.collect::<Vec<_>>();
            if is_help_request(&args) {
                print_help(DOCTOR_HELP);
                return Ok(0);
            }
            let config = parse_doctor_args(args.into_iter())?;
            doctor(&config)
        }
        Some("status") => {
            let args = args.collect::<Vec<_>>();
            if is_help_request(&args) {
                print_help(STATUS_HELP);
                return Ok(0);
            }
            let config = parse_status_args(args.into_iter())?;
            status(&config)
        }
        Some("dump") => {
            let args = args.collect::<Vec<_>>();
            if is_help_request(&args) {
                print_help(DUMP_HELP);
                return Ok(0);
            }
            let args = parse_dump_args(args.into_iter())?;
            let device_path = args.device.resolve(&args.input_root)?;
            dump(&device_path, &args.out, args.frames, args.raw).map(|()| 0)
        }
        Some("proxy") => {
            let args = args.collect::<Vec<_>>();
            if is_help_request(&args) {
                print_help(PROXY_HELP);
                return Ok(0);
            }
            let args = parse_proxy_args(args.into_iter())?;
            proxy(&args).map(|()| 0)
        }
        Some("daemon") => {
            let args = args.collect::<Vec<_>>();
            if is_help_request(&args) {
                print_help(DAEMON_HELP);
                return Ok(0);
            }
            let args = parse_daemon_args(args.into_iter())?;
            daemon(&args).map(|()| 0)
        }
        _ => Err(USAGE.to_string()),
    }
}

fn is_help_request(args: &[String]) -> bool {
    args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h")
}

fn print_help(help: &str) {
    print!("{help}");
}
