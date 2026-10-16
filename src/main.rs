mod app;

use std::process;

use app::run;

fn main() {
    match run() {
        Ok(exit_code) => process::exit(exit_code),
        Err(err) => {
            eprintln!("{err}");
            process::exit(1);
        }
    }
}
