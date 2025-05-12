use std::ffi::OsString;

use clap::{Args, Parser, Subcommand};
use clap_verbosity_flag::InfoLevel;

/// Rewatch is an alternative build system for the Rescript Compiler bsb (which uses Ninja internally). It strives
/// to deliver consistent and faster builds in monorepo setups with multiple packages, where the
/// default build system fails to pick up changed interfaces across multiple packages.
#[derive(Parser, Debug)]
#[command(version)]
#[command(args_conflicts_with_subcommands = true)]
pub struct Cli {
    /// Verbosity:
    /// -v -> Debug
    /// -vv -> Trace
    /// -q -> Warn
    /// -qq -> Error
    /// -qqq -> Off.
    /// Default (/ no argument given): 'info'
    #[command(flatten)]
    pub verbose: clap_verbosity_flag::Verbosity<InfoLevel>,

    /// The command to run. If not provided it will default to build.
    #[command(subcommand)]
    pub command: Option<Command>,

    /// The relative path to where the main rescript.json resides. IE - the root of your project.
    #[arg(default_value = ".")]
    pub folder: String,

    #[command(flatten)]
    pub build_args: BuildArgs,
}

#[derive(Args, Debug, Clone)]
pub struct BuildArgs {
    /// Filter files by regex
    ///
    /// Filter allows for a regex to be supplied which will filter the files to be compiled. For
    /// instance, to filter out test files for compilation while doing feature work.
    #[arg(short, long)]
    pub filter: Option<String>,

    /// Action after build
    ///
    /// This allows one to pass an additional command to the watcher, which allows it to run when
    /// finished. For instance, to play a sound when done compiling, or to run a test suite.
    /// NOTE - You may need to add '--color=always' to your subcommand in case you want to output
    /// colour as well
    #[arg(short, long)]
    pub after_build: Option<String>,

    /// Create source_dirs.json
    ///
    /// This creates a source_dirs.json file at the root of the monorepo, which is needed when you
    /// want to use Reanalyze
    #[arg(short, long, default_value_t = false, num_args = 0..=1)]
    pub create_sourcedirs: bool,

    /// Build development dependencies
    ///
    /// This is the flag to also compile development dependencies
    /// It's important to know that we currently do not discern between project src, and
    /// dependencies. So enabling this flag will enable building _all_ development dependencies of
    /// _all_ packages
    #[arg(long, default_value_t = false, num_args = 0..=1)]
    pub dev: bool,

    /// Disable timing on the output
    #[arg(short, long, default_value_t = false, num_args = 0..=1)]
    pub no_timing: bool,

    /// Path to bsc
    #[arg(long)]
    pub bsc_path: Option<String>,
}

#[derive(Args, Debug)]
pub struct WatchArgs {
    /// Filter files by regex
    ///
    /// Filter allows for a regex to be supplied which will filter the files to be compiled. For
    /// instance, to filter out test files for compilation while doing feature work.
    #[arg(short, long)]
    pub filter: Option<String>,

    /// Action after build
    ///
    /// This allows one to pass an additional command to the watcher, which allows it to run when
    /// finished. For instance, to play a sound when done compiling, or to run a test suite.
    /// NOTE - You may need to add '--color=always' to your subcommand in case you want to output
    /// colour as well
    #[arg(short, long)]
    pub after_build: Option<String>,

    /// Create source_dirs.json
    ///
    /// This creates a source_dirs.json file at the root of the monorepo, which is needed when you
    /// want to use Reanalyze
    #[arg(short, long, default_value_t = false, num_args = 0..=1)]
    pub create_sourcedirs: bool,

    /// Build development dependencies
    ///
    /// This is the flag to also compile development dependencies
    /// It's important to know that we currently do not discern between project src, and
    /// dependencies. So enabling this flag will enable building _all_ development dependencies of
    /// _all_ packages
    #[arg(long, default_value_t = false, num_args = 0..=1)]
    pub dev: bool,

    /// Path to bsc
    #[arg(long)]
    pub bsc_path: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Build using Rewatch
    Build(BuildArgs),
    /// Build, then start a watcher
    Watch(WatchArgs),
    /// Clean the build artifacts
    Clean {
        /// Path to bsc
        #[arg(long)]
        bsc_path: Option<String>,
    },
    /// Alias to `legacy format`.
    #[command(disable_help_flag = true)]
    Format {
        #[arg(allow_hyphen_values = true, num_args = 0..)]
        format_args: Vec<OsString>,
    },
    /// Alias to `legacy dump`.
    #[command(disable_help_flag = true)]
    Dump {
        #[arg(allow_hyphen_values = true, num_args = 0..)]
        dump_args: Vec<OsString>,
    },
    /// This prints the compiler arguments. It expects the path to a rescript.json file.
    CompilerArgs {
        /// Path to a rescript.json file
        #[command()]
        path: String,

        #[arg(long, default_value_t = false, num_args = 0..=1)]
        dev: bool,

        /// To be used in conjunction with compiler_args
        #[arg(long)]
        rescript_version: Option<String>,

        /// A custom path to bsc
        #[arg(long)]
        bsc_path: Option<String>,
    },
    /// Use the legacy build system.
    ///
    /// After this command is encountered, the rest of the arguments are passed to the legacy build system.
    #[command(disable_help_flag = true)]
    Legacy {
        #[arg(allow_hyphen_values = true, num_args = 0..)]
        legacy_args: Vec<OsString>,
    },
}
