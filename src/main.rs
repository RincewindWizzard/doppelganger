mod database;

mod file_walker;

use crate::database::Database;
use clap::Parser;
use log::{debug, error, info};
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "envbuddel")]
#[command(about = "Hash indexer for files", long_about = None)]
#[command(version = env!("CARGO_PKG_VERSION"))]
struct Cli {
    /// Increase verbosity (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// path to folder that will be indexed
    #[arg(default_value = ".")]
    dst: PathBuf,

    /// path to database file where result is stored
    #[arg(long, default_value = "./doppelganger.db")]
    database: PathBuf,
}

fn init_logger(verbosity: u8) {
    use env_logger::{Builder, Target};
    use std::io::Write;

    Builder::new()
        .format(|buf, record| {
            let msg = format!("{}", record.args());
            match record.level() {
                log::Level::Warn => writeln!(buf, "\x1b[33m[WARN] {}\x1b[0m", msg), // red
                log::Level::Error => writeln!(buf, "\x1b[91m[ERROR] {}\x1b[0m", msg), // bright red
                _ => writeln!(buf, "{}", msg),                                      // default
            }
        })
        .target(Target::Stdout)
        .filter_level(match verbosity {
            0 => log::LevelFilter::Info,  // always show info & higher
            1 => log::LevelFilter::Debug, // debug + info + warn + error
            _ => log::LevelFilter::Trace, // trace + debug + info + warn + error
        })
        .init();
}

fn main() {
    let cli = Cli::parse();
    init_logger(cli.verbose);

    debug!("{:?}", cli);
    if let Err(err) = run(cli) {
        error!("{}", err);
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new(&cli.database)?;

    file_walker::walk_and_hash(db, cli.dst)?;

    Ok(())
}
