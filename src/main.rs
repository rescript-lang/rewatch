pub mod bsconfig;
pub mod build;
pub mod grouplike;
pub mod helpers;
pub mod package_tree;
pub mod structure_hashmap;
pub mod watcher;
use crate::grouplike::*;
use ahash::AHashSet;
use console::{style, Emoji};
use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use log::Level::Info;
use log::{info, log_enabled};
use rayon::prelude::*;
use std::io::stdout;
use std::io::Write;
use std::time::Instant;

fn clean() {
    let project_root = helpers::get_abs_path("walnut_monorepo");
    let packages = package_tree::make(&project_root);

    packages.iter().for_each(|(_, package)| {
        println!("Cleaning {}...", package.name);
        let path = std::path::Path::new(&package.package_dir)
            .join("lib")
            .join("bs");
        let _ = std::fs::remove_dir_all(path);
    })
}

static TREE: Emoji<'_, '_> = Emoji("🌴 ", "");
static LOOKING_GLASS: Emoji<'_, '_> = Emoji("🔍 ", "");
static CODE: Emoji<'_, '_> = Emoji("🟰  ", "");
static SWORDS: Emoji<'_, '_> = Emoji("⚔️  ", "");
static CHECKMARK: Emoji<'_, '_> = Emoji("️✅  ", "");
static CROSS: Emoji<'_, '_> = Emoji("️🛑  ", "");
static LINE_CLEAR: &str = "\x1b[2K";

fn build() {
    let timing_total = Instant::now();
    env_logger::init();
    let project_root = helpers::get_abs_path("walnut_monorepo");
    let rescript_version = build::get_version(&project_root);

    print!(
        "{} {} Building package tree...",
        style("[1/4]").bold().dim(),
        TREE
    );
    let _ = stdout().flush();
    let timing_package_tree = Instant::now();
    let packages = package_tree::make(&project_root);
    let timing_package_tree_elapsed = timing_package_tree.elapsed();
    println!(
        "{}\r{} {}Built package tree in {:.2}s",
        LINE_CLEAR,
        style("[1/4]").bold().dim(),
        CHECKMARK,
        timing_package_tree_elapsed.as_secs_f64()
    );

    let timing_source_files = Instant::now();
    print!(
        "{} {} Finding source files...",
        style("[2/4]").bold().dim(),
        LOOKING_GLASS
    );
    let _ = stdout().flush();
    let (all_modules, modules) = build::parse(&project_root, packages.to_owned());
    let timing_source_files_elapsed = timing_source_files.elapsed();
    println!(
        "{}\r{} {}Found source files in {:.2}s",
        LINE_CLEAR,
        style("[2/4]").bold().dim(),
        CHECKMARK,
        timing_source_files_elapsed.as_secs_f64()
    );
    print!(
        "{} {} Parsing source files...",
        style("[3/4]").bold().dim(),
        CODE
    );
    let _ = stdout().flush();

    let timing_ast = Instant::now();
    let modules = build::generate_asts(
        rescript_version.to_string(),
        &project_root,
        modules,
        all_modules,
    );
    let timing_ast_elapsed = timing_ast.elapsed();
    println!(
        "{}\r{} {}Parsed source files in {:.2}s",
        LINE_CLEAR,
        style("[3/4]").bold().dim(),
        CHECKMARK,
        timing_ast_elapsed.as_secs_f64()
    );

    let pb = ProgressBar::new(modules.len().try_into().unwrap());
    pb.set_style(
        ProgressStyle::with_template(&format!(
            "{} {} Compiling... {{wide_bar}} {{pos}}/{{len}} {{msg}}",
            style("[4/4]").bold().dim(),
            SWORDS
        ))
        .unwrap(),
    );
    let start_compiling = Instant::now();

    let mut compiled_modules = AHashSet::<String>::new();

    let mut loop_count = 0;
    let mut files_total_count = 0;
    let mut files_current_loop_count = -1;
    let mut compile_errors = "".to_string();

    while files_current_loop_count != 0 {
        files_current_loop_count = 0;
        loop_count += 1;

        info!(
            "Compiled: {} out of {}. Compile loop: {}",
            files_total_count,
            modules.len(),
            loop_count,
        );

        modules
            .par_iter()
            .map(|(module_name, module)| {
                let mut stderr = None;
                if module.deps.is_subset(&compiled_modules)
                    && !compiled_modules.contains(module_name)
                {
                    match module.source_type.to_owned() {
                        build::SourceType::MlMap => (Some(module_name.to_owned()), None),
                        build::SourceType::SourceFile => {
                            // compile interface first
                            match module.asti_path.to_owned() {
                                Some(asti_path) => {
                                    let asti_err = build::compile_file(
                                        &module.package.name,
                                        &asti_path,
                                        module,
                                        &project_root,
                                        true,
                                    );
                                    stderr = stderr.mappend(asti_err);
                                }
                                _ => (),
                            }

                            let ast_err = build::compile_file(
                                &module.package.name,
                                &module.ast_path.to_owned().unwrap(),
                                module,
                                &project_root,
                                false,
                            );

                            (Some(module_name.to_owned()), stderr.mappend(ast_err))
                        }
                    }
                } else {
                    (None, None)
                }
            })
            .collect::<Vec<(Option<String>, Option<String>)>>()
            .iter()
            .for_each(|(module_name, stderr)| {
                module_name.iter().for_each(|name| {
                    if !(log_enabled!(Info)) {
                        pb.inc(1);
                    }
                    files_current_loop_count += 1;
                    compiled_modules.insert(name.to_string());
                });

                stderr.iter().for_each(|err| {
                    compile_errors.push_str(err);
                    // error!("Some error were generated compiling this round: \n {}", err);
                })
            });

        if files_current_loop_count == 0 {
            // we probably want to find the cycle(s), and give a helpful error message here
            compile_errors.push_str("Can't continue... Dependency cycle\n")
        }

        if compile_errors.len() > 0 {
            break;
        };

        files_total_count += files_current_loop_count;
    }
    let compile_duration = start_compiling.elapsed();

    pb.finish();
    if compile_errors.len() > 0 {
        println!(
            "{}\r{} {}Compiled in {:.2}s",
            LINE_CLEAR,
            style("[4/4]").bold().dim(),
            CROSS,
            compile_duration.as_secs_f64()
        );
        println!("{}", &compile_errors);
        std::process::exit(1);
    }
    println!(
        "{}\r{} {}Compiled in {:.2}s",
        LINE_CLEAR,
        style("[4/4]").bold().dim(),
        CHECKMARK,
        compile_duration.as_secs_f64()
    );
    let timing_total_elapsed = timing_total.elapsed();
    println!("Done in {:.2}s", timing_total_elapsed.as_secs_f64());
}

fn main() {
    let command = std::env::args().nth(1).unwrap_or("build".to_string());
    match command.as_str() {
        "clean" => clean(),
        "build" => build(),
        _ => println!("Not a valid build command"),
    }
}
