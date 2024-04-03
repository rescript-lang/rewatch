pub mod build_types;
pub mod clean;
pub mod compile;
pub mod deps;
pub mod logs;
pub mod namespaces;
pub mod packages;
pub mod parse;
pub mod read_compile_state;

use crate::build::compile::{mark_modules_with_deleted_deps_dirty, mark_modules_with_expired_deps_dirty};
use crate::helpers::emojis::*;
use crate::helpers::{self, get_workspace_root};
use ahash::AHashSet;
use build_types::*;
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use serde::Serialize;
use std::io::{stdout, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use self::compile::compiler_args;
use self::parse::parser_args;

fn is_dirty(module: &Module) -> bool {
    match module.source_type {
        SourceType::SourceFile(SourceFile {
            implementation: Implementation {
                parse_dirty: true, ..
            },
            ..
        }) => true,
        SourceType::SourceFile(SourceFile {
            interface: Some(Interface {
                parse_dirty: true, ..
            }),
            ..
        }) => true,
        SourceType::SourceFile(_) => false,
        SourceType::MlMap(MlMap {
            parse_dirty: dirty, ..
        }) => dirty,
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct CompilerArgs {
    pub compiler_args: Vec<String>,
    pub parser_args: Vec<String>,
}

pub fn get_compiler_args(path: &str) -> String {
    let filename = &helpers::get_abs_path(path);
    let package_root = helpers::get_abs_path(
        &helpers::get_nearest_bsconfig(&std::path::PathBuf::from(path)).expect("Couldn't find package root"),
    );
    let workspace_root = get_workspace_root(&package_root).map(|p| helpers::get_abs_path(&p));
    let root_config_name =
        packages::get_package_name(&workspace_root.to_owned().unwrap_or(package_root.to_owned()));
    let package_name = packages::get_package_name(&package_root);
    let bsc_path = helpers::get_bsc(&package_root, workspace_root.to_owned());
    let rescript_version = helpers::get_rescript_version(&bsc_path);
    let packages = packages::make(
        &None,
        &workspace_root.to_owned().unwrap_or(package_root.to_owned()),
        &workspace_root,
    );
    // make PathBuf from package root and get the relative path for filename
    let relative_filename = PathBuf::from(&filename)
        .strip_prefix(PathBuf::from(&package_root).parent().unwrap())
        .unwrap()
        .to_string_lossy()
        .to_string();
    let root_package = packages.get(&root_config_name).unwrap();
    let package = packages.get(&package_name).unwrap();
    let (ast_path, parser_args) = parser_args(
        package,
        root_package,
        &relative_filename,
        &rescript_version,
        &workspace_root,
    );
    let is_interface = filename.ends_with("i");
    let has_interface = if is_interface {
        true
    } else {
        let mut interface_filename = filename.to_string();
        interface_filename.push('i');
        PathBuf::from(&interface_filename).exists()
    };
    let compiler_args = compiler_args(
        package,
        root_package,
        &ast_path,
        &rescript_version,
        &relative_filename,
        is_interface,
        has_interface,
        &packages,
    );
    serde_json::to_string_pretty(&CompilerArgs {
        compiler_args,
        parser_args,
    })
    .unwrap()
}

pub fn initialize_build<'a>(
    default_timing: Option<Duration>,
    filter: &Option<regex::Regex>,
    path: &str,
) -> Result<BuildState, ()> {
    let project_root = helpers::get_abs_path(path);
    let workspace_root = helpers::get_workspace_root(&project_root);
    let bsc_path = helpers::get_bsc(&project_root, workspace_root.to_owned());
    let root_config_name = packages::get_package_name(&project_root);
    let rescript_version = helpers::get_rescript_version(&bsc_path);

    print!(
        "{} {} Building package tree...",
        style("[1/7]").bold().dim(),
        TREE
    );
    let _ = stdout().flush();
    let timing_package_tree = Instant::now();
    let packages = packages::make(&filter, &project_root, &workspace_root);
    let timing_package_tree_elapsed = timing_package_tree.elapsed();

    println!(
        "{}\r{} {}Built package tree in {:.2}s",
        LINE_CLEAR,
        style("[1/7]").bold().dim(),
        CHECKMARK,
        default_timing
            .unwrap_or(timing_package_tree_elapsed)
            .as_secs_f64()
    );

    if !packages::validate_packages_dependencies(&packages) {
        return Err(());
    }

    let timing_source_files = Instant::now();

    print!(
        "{} {} Finding source files...",
        style("[2/7]").bold().dim(),
        LOOKING_GLASS
    );
    let _ = stdout().flush();
    let mut build_state = BuildState::new(
        project_root,
        root_config_name,
        packages,
        workspace_root,
        rescript_version,
        bsc_path,
    );
    packages::parse_packages(&mut build_state);
    let timing_source_files_elapsed = timing_source_files.elapsed();
    println!(
        "{}\r{} {}Found source files in {:.2}s",
        LINE_CLEAR,
        style("[2/7]").bold().dim(),
        CHECKMARK,
        default_timing
            .unwrap_or(timing_source_files_elapsed)
            .as_secs_f64()
    );

    print!(
        "{} {} Reading compile state...",
        style("[3/7]").bold().dim(),
        LOOKING_GLASS
    );
    let _ = stdout().flush();
    let timing_compile_state = Instant::now();
    let compile_assets_state = read_compile_state::read(&mut build_state);
    let timing_compile_state_elapsed = timing_compile_state.elapsed();
    println!(
        "{}\r{} {}Read compile state {:.2}s",
        LINE_CLEAR,
        style("[3/7]").bold().dim(),
        CHECKMARK,
        default_timing
            .unwrap_or(timing_compile_state_elapsed)
            .as_secs_f64()
    );

    print!(
        "{} {} Cleaning up previous build...",
        style("[4/7]").bold().dim(),
        SWEEP
    );
    let timing_cleanup = Instant::now();
    let (diff_cleanup, total_cleanup) = clean::cleanup_previous_build(&mut build_state, compile_assets_state);
    let timing_cleanup_elapsed = timing_cleanup.elapsed();
    println!(
        "{}\r{} {}Cleaned {}/{} {:.2}s",
        LINE_CLEAR,
        style("[4/7]").bold().dim(),
        CHECKMARK,
        diff_cleanup,
        total_cleanup,
        default_timing.unwrap_or(timing_cleanup_elapsed).as_secs_f64()
    );
    Ok(build_state)
}

pub fn incremental_build(
    build_state: &mut BuildState,
    default_timing: Option<Duration>,
    initial_build: bool,
) -> Result<(), ()> {
    logs::initialize(&build_state.packages);
    let num_dirty_modules = build_state.modules.values().filter(|m| is_dirty(m)).count() as u64;

    let pb = ProgressBar::new(num_dirty_modules);
    pb.set_style(
        ProgressStyle::with_template(&format!(
            "{} {} Parsing... {{spinner}} {{pos}}/{{len}} {{msg}}",
            style("[5/7]").bold().dim(),
            CODE
        ))
        .unwrap(),
    );

    let timing_ast = Instant::now();
    let result_asts = parse::generate_asts(build_state, || pb.inc(1));
    let timing_ast_elapsed = timing_ast.elapsed();

    match result_asts {
        Ok(err) => {
            println!(
                "{}\r{} {}Parsed {} source files in {:.2}s",
                LINE_CLEAR,
                style("[5/7]").bold().dim(),
                CHECKMARK,
                num_dirty_modules,
                default_timing.unwrap_or(timing_ast_elapsed).as_secs_f64()
            );
            print!("{}", &err);
        }
        Err(err) => {
            logs::finalize(&build_state.packages);
            println!(
                "{}\r{} {}Error parsing source files in {:.2}s",
                LINE_CLEAR,
                style("[5/7]").bold().dim(),
                CROSS,
                default_timing.unwrap_or(timing_ast_elapsed).as_secs_f64()
            );
            print!("{}", &err);
            return Err(());
        }
    }
    let timing_deps = Instant::now();
    deps::get_deps(build_state, &build_state.deleted_modules.to_owned());
    let timing_deps_elapsed = timing_deps.elapsed();

    println!(
        "{}\r{} {}Collected deps in {:.2}s",
        LINE_CLEAR,
        style("[6/7]").bold().dim(),
        CHECKMARK,
        default_timing.unwrap_or(timing_deps_elapsed).as_secs_f64()
    );

    // track the compile dirty state, we reset it when the compile fails
    let mut tracked_dirty_modules = AHashSet::new();
    for (module_name, module) in build_state.modules.iter() {
        if module.compile_dirty {
            tracked_dirty_modules.insert(module_name.to_owned());
        }
    }
    if initial_build {
        // repair broken state
        mark_modules_with_expired_deps_dirty(build_state);
    }
    mark_modules_with_deleted_deps_dirty(build_state);

    // print all the compile_dirty modules
    // for (module_name, module) in build_state.modules.iter() {
    //     if module.compile_dirty {
    //         println!("compile dirty: {}", module_name);
    //     }
    // }

    let start_compiling = Instant::now();
    let pb = ProgressBar::new(build_state.modules.len().try_into().unwrap());
    pb.set_style(
        ProgressStyle::with_template(&format!(
            "{} {} Compiling... {{spinner}} {{pos}}/{{len}} {{msg}}",
            style("[7/7]").bold().dim(),
            SWORDS
        ))
        .unwrap(),
    );
    let (compile_errors, compile_warnings, num_compiled_modules) =
        compile::compile(build_state, || pb.inc(1), |size| pb.set_length(size));
    let compile_duration = start_compiling.elapsed();

    logs::finalize(&build_state.packages);
    pb.finish();
    if compile_errors.len() > 0 {
        if helpers::contains_ascii_characters(&compile_warnings) {
            println!("{}", &compile_warnings);
        }
        println!(
            "{}\r{} {}Compiled {} modules in {:.2}s",
            LINE_CLEAR,
            style("[7/7]").bold().dim(),
            CROSS,
            num_compiled_modules,
            default_timing.unwrap_or(compile_duration).as_secs_f64()
        );
        print!("{}", &compile_errors);
        // mark the original files as dirty again, because we didn't complete a full build
        for (module_name, module) in build_state.modules.iter_mut() {
            if tracked_dirty_modules.contains(module_name) {
                module.compile_dirty = true;
            }
        }
        return Err(());
    } else {
        println!(
            "{}\r{} {}Compiled {} modules in {:.2}s",
            LINE_CLEAR,
            style("[7/7]").bold().dim(),
            CHECKMARK,
            num_compiled_modules,
            default_timing.unwrap_or(compile_duration).as_secs_f64()
        );
        if helpers::contains_ascii_characters(&compile_warnings) {
            print!("{}", &compile_warnings);
        }
        Ok(())
    }
}

pub fn build(filter: &Option<regex::Regex>, path: &str, no_timing: bool) -> Result<BuildState, ()> {
    let default_timing: Option<std::time::Duration> = if no_timing {
        Some(std::time::Duration::new(0.0 as u64, 0.0 as u32))
    } else {
        None
    };
    let timing_total = Instant::now();
    let mut build_state = initialize_build(default_timing, filter, path)?;
    match incremental_build(&mut build_state, default_timing, true) {
        Ok(_) => {
            let timing_total_elapsed = timing_total.elapsed();
            println!(
                "{}\r{}Finished Compilation in {:.2}s",
                LINE_CLEAR,
                CHECKMARK,
                default_timing.unwrap_or(timing_total_elapsed).as_secs_f64()
            );
            clean::cleanup_after_build(&build_state);
            Ok(build_state)
        }
        Err(_) => {
            clean::cleanup_after_build(&build_state);
            Err(())
        }
    }
}
