use clap::{Parser, ValueEnum};
use regex::Regex;

pub mod bsconfig;
pub mod build;
pub mod cmd;
pub mod helpers;
pub mod lock;
pub mod queue;
pub mod watcher;

#[derive(Debug, Clone, ValueEnum)]
enum Command {
    /// Build using Rewatch
    Build,
    /// Build, then start a watcher
    Watch,
    /// Clean the build artifacts
    Clean,
}

/// Rewatch is an alternative build system for the Rescript Compiler bsb (which uses Ninja internally). It strives
/// to deliver consistent and faster builds in monorepo setups with multiple packages, where the
/// default build system fails to pick up changed interfaces across multiple packages.
#[derive(Parser, Debug)]
#[command(version)]
struct Args {
    #[arg(value_enum)]
    command: Option<Command>,

    /// The relative path to where the main bsconfig.json resides. IE - the root of your project.
    folder: Option<String>,

    /// Filter allows for a regex to be supplied which will filter the files to be compiled. For
    /// instance, to filter out test files for compilation while doing feature work.
    #[arg(short, long)]
    filter: Option<String>,

    /// This allows one to pass an additional command to the watcher, which allows it to run when
    /// finished. For instance, to play a sound when done compiling, or to run a test suite.
    /// NOTE - You may need to add '--color=always' to your subcommand in case you want to output
    /// colour as well
    #[arg(short, long)]
    after_build: Option<String>,

    #[arg(short, long)]
    no_timing: Option<bool>,

    #[arg(long)]
    compiler_args: Option<String>,
}

fn main() {
    env_logger::init();
    let args = Args::parse();

    let command = args.command.unwrap_or(Command::Build);
    let folder = args.folder.unwrap_or(".".to_string());
    let filter = args
        .filter
        .map(|filter| Regex::new(filter.as_ref()).expect("Could not parse regex"));

    match args.compiler_args {
        None => (),
        Some(path) => {
            println!("{}", build::get_compiler_args(&path));
            std::process::exit(0);
        }
    }

    match lock::get(&folder) {
        lock::Lock::Error(ref e) => {
            eprintln!("Error while trying to get lock: {}", e.to_string());
            std::process::exit(1)
        }
        lock::Lock::Aquired(_) => match command {
            Command::Clean => build::clean::clean(&folder),
            Command::Build => {
                match build::build(&filter, &folder, args.no_timing.unwrap_or(false)) {
                    Err(()) => std::process::exit(1),
                    Ok(_) => {
                        args.after_build.map(|command| cmd::run(command));
                        std::process::exit(0)
                    }
                };
            }
            Command::Watch => {
                let _initial_build = build::build(&filter, &folder, false);
                args.after_build.clone().map(|command| cmd::run(command));
                watcher::start(&filter, &folder, args.after_build);
            }
        },
    }
}
