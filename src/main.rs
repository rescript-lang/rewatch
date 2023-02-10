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
use log::Level::Info;
use log::{error, info, log_enabled};
use rayon::prelude::*;

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
static NUMBERS: Emoji<'_, '_> = Emoji("🔢 ", "");
static CODE: Emoji<'_, '_> = Emoji("🟰  ", "");
static SWORDS: Emoji<'_, '_> = Emoji("⚔️  ", "");

fn build() {
    env_logger::init();
    let project_root = helpers::get_abs_path("walnut_monorepo");

    println!(
        "{} {} Getting Version Number",
        style("[1/5]").bold().dim(),
        NUMBERS
    );
    let rescript_version = build::get_version(&project_root);

    println!(
        "{} {} Building Package Tree...",
        style("[2/5]").bold().dim(),
        TREE
    );
    let packages = package_tree::make(&project_root);

    println!(
        "{} {} Finding Source Files...",
        style("[3/5]").bold().dim(),
        LOOKING_GLASS
    );
    let (all_modules, modules) = build::parse(&project_root, packages.to_owned());

    println!(
        "{} {} Generating AST's...",
        style("[4/5]").bold().dim(),
        CODE
    );
    let modules = build::generate_asts(
        rescript_version.to_string(),
        &project_root,
        modules,
        all_modules,
    );

    // let all_modules = modules
    //     .keys()
    //     .map(|key| key.to_owned())
    //     .collect::<AHashSet<String>>();

    println!("{} {} Compiling...", style("[5/5]").bold().dim(), SWORDS);
    let pb = ProgressBar::new(modules.len().try_into().unwrap());

    let mut compiled_modules = AHashSet::<String>::new();

    let mut loop_count = 0;
    let mut files_total_count = 0;
    let mut files_current_loop_count = -1;

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
                    error!("Some error were generated compiling this round: \n {}", err);
                })
            });

        files_total_count += files_current_loop_count;
    }

    pb.finish_with_message("Finished Compiling...");
}

fn main() {
    let command = std::env::args().nth(1).unwrap_or("build".to_string());
    match command.as_str() {
        "clean" => clean(),
        "build" => build(),
        _ => println!("Not a valid build command"),
    }
}

fn main() {
    let command = std::env::args().nth(1).unwrap_or("build".to_string());
    match command.as_str() {
        "clean" => clean(),
        "build" => build(),
        _ => println!("Not a valid build command"),
    }
}
