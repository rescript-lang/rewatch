use super::build_types::*;
use super::logs;
use super::namespaces;
use super::packages;
use crate::bsconfig;
use crate::bsconfig::OneOrMore;
use crate::helpers;
use ahash::AHashSet;
use log::debug;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn generate_asts(
    build_state: &mut BuildState,
    inc: impl Fn() -> () + std::marker::Sync,
) -> Result<String, String> {
    let mut has_failure = false;
    let mut stderr = "".to_string();

    let results = build_state
        .modules
        .par_iter()
        .map(|(module_name, module)| {
            debug!("Generating AST for module: {}", module_name);
            let package = build_state
                .get_package(&module.package_name)
                .expect("Package not found");
            match &module.source_type {
                SourceType::MlMap(_mlmap) => {
                    let path = package.get_mlmap_path();
                    (module_name.to_owned(), Ok((path, None)), Ok(None), false)
                }

                SourceType::SourceFile(source_file) => {
                    let root_package = build_state.get_package(&build_state.root_config_name).unwrap();

                    let (ast_path, iast_path, dirty) = if source_file.implementation.parse_dirty
                        || source_file
                            .interface
                            .as_ref()
                            .map(|i| i.parse_dirty)
                            .unwrap_or(false)
                    {
                        inc();
                        let ast_result = generate_ast(
                            package.to_owned(),
                            root_package.to_owned(),
                            &source_file.implementation.path.to_owned(),
                            &build_state.rescript_version,
                            &build_state.bsc_path,
                            &build_state.workspace_root,
                        );

                        let iast_result = match source_file.interface.as_ref().map(|i| i.path.to_owned()) {
                            Some(interface_file_path) => generate_ast(
                                package.to_owned(),
                                root_package.to_owned(),
                                &interface_file_path.to_owned(),
                                &build_state.rescript_version,
                                &build_state.bsc_path,
                                &build_state.workspace_root,
                            )
                            .map(|result| Some(result)),
                            _ => Ok(None),
                        };

                        (ast_result, iast_result, true)
                    } else {
                        (
                            Ok((
                                helpers::get_basename(&source_file.implementation.path).to_string() + ".ast",
                                None,
                            )),
                            Ok(source_file
                                .interface
                                .as_ref()
                                .map(|i| (helpers::get_basename(&i.path).to_string() + ".iast", None))),
                            false,
                        )
                    };

                    (module_name.to_owned(), ast_path, iast_path, dirty)
                }
            }
        })
        .collect::<Vec<(
            String,
            Result<(String, Option<String>), String>,
            Result<Option<(String, Option<String>)>, String>,
            bool,
        )>>();

    results
        .into_iter()
        .for_each(|(module_name, ast_path, iast_path, is_dirty)| {
            if let Some(module) = build_state.modules.get_mut(&module_name) {
                let package = build_state
                    .packages
                    .get(&module.package_name)
                    .expect("Package not found");
                if is_dirty {
                    module.compile_dirty = true
                }
                match ast_path {
                    Ok((_path, err)) => {
                        match module.source_type {
                            SourceType::SourceFile(ref mut source_file) => {
                                source_file.implementation.parse_dirty = false;
                                source_file
                                    .interface
                                    .as_mut()
                                    .map(|interface| interface.parse_dirty = false);
                            }
                            _ => (),
                        }
                        // supress warnings in non-pinned deps
                        match module.source_type {
                            SourceType::SourceFile(ref mut source_file) => {
                                source_file.implementation.parse_state = ParseState::Success;
                            }
                            _ => (),
                        }

                        if package.is_pinned_dep {
                            if let Some(err) = err {
                                match module.source_type {
                                    SourceType::SourceFile(ref mut source_file) => {
                                        source_file.implementation.parse_state = ParseState::Warning;
                                        source_file.implementation.parse_dirty = true;
                                    }
                                    _ => (),
                                }
                                logs::append(package, &err);
                                stderr.push_str(&err);
                            }
                        }
                    }
                    Err(err) => {
                        match module.source_type {
                            SourceType::SourceFile(ref mut source_file) => {
                                source_file.implementation.parse_state = ParseState::ParseError;
                                source_file.implementation.parse_dirty = true;
                            }
                            _ => (),
                        }
                        logs::append(package, &err);
                        has_failure = true;
                        stderr.push_str(&err);
                    }
                };
                match iast_path {
                    Ok(Some((_path, err))) => {
                        // supress warnings in non-pinned deps
                        if package.is_pinned_dep {
                            if let Some(err) = err {
                                match module.source_type {
                                    SourceType::SourceFile(ref mut source_file) => {
                                        source_file.interface.as_mut().map(|interface| {
                                            interface.parse_state = ParseState::ParseError;
                                            interface.parse_dirty = true;
                                        });
                                    }
                                    _ => (),
                                }
                                logs::append(package, &err);
                                stderr.push_str(&err);
                            }
                        }
                    }
                    Ok(None) => (),
                    Err(err) => {
                        match module.source_type {
                            SourceType::SourceFile(ref mut source_file) => {
                                source_file.interface.as_mut().map(|interface| {
                                    interface.parse_state = ParseState::ParseError;
                                    interface.parse_dirty = true;
                                });
                            }
                            _ => (),
                        }
                        logs::append(package, &err);
                        has_failure = true;
                        stderr.push_str(&err);
                    }
                };
            }
        });

    // compile the mlmaps of dirty modules
    // first collect dirty packages
    let dirty_packages = build_state
        .modules
        .iter()
        .filter(|(_, module)| module.compile_dirty)
        .map(|(_, module)| module.package_name.clone())
        .collect::<AHashSet<String>>();

    build_state.modules.iter_mut().for_each(|(module_name, module)| {
        let is_dirty = match &module.source_type {
            SourceType::MlMap(_) => {
                if dirty_packages.contains(&module.package_name) {
                    let package = build_state
                        .packages
                        .get(&module.package_name)
                        .expect("Package not found");
                    // probably better to do this in a different function
                    // specific to compiling mlmaps
                    let compile_path = package.get_mlmap_compile_path();
                    let mlmap_hash = helpers::compute_file_hash(&compile_path);
                    namespaces::compile_mlmap(&package, module_name, &build_state.bsc_path);
                    let mlmap_hash_after = helpers::compute_file_hash(&compile_path);

                    match (mlmap_hash, mlmap_hash_after) {
                        (Some(digest), Some(digest_after)) => !digest.eq(&digest_after),
                        _ => true,
                    }
                } else {
                    false
                }
            }
            _ => false,
        };
        if is_dirty {
            module.compile_dirty = is_dirty;
        }
    });

    if has_failure {
        Err(stderr)
    } else {
        Ok(stderr)
    }
}

pub fn parser_args(
    config: &bsconfig::Config,
    root_config: &bsconfig::Config,
    filename: &str,
    version: &str,
    workspace_root: &Option<String>,
    root_path: &str,
) -> (String, Vec<String>) {
    let file = &filename.to_string();
    let path = PathBuf::from(filename);
    let ast_extension = path_to_ast_extension(&path);
    let ast_path = (helpers::get_basename(&file.to_string()).to_owned()) + ast_extension;
    let ppx_flags = bsconfig::flatten_ppx_flags(
        &if let Some(workspace_root) = workspace_root {
            format!("{}/node_modules", &workspace_root)
        } else {
            format!("{}/node_modules", &root_path)
        },
        &filter_ppx_flags(&config.ppx_flags),
        &config.name,
    );
    let jsx_args = root_config.get_jsx_args();
    let jsx_module_args = root_config.get_jsx_module_args();
    let jsx_mode_args = root_config.get_jsx_mode_args();
    let uncurried_args = root_config.get_uncurried_args(version);
    let bsc_flags = bsconfig::flatten_flags(&config.bsc_flags);

    let file = "../../".to_string() + file;
    (
        ast_path.to_string(),
        vec![
            vec!["-bs-v".to_string(), format!("{}", version)],
            ppx_flags,
            jsx_args,
            jsx_module_args,
            jsx_mode_args,
            uncurried_args,
            bsc_flags,
            vec![
                "-absname".to_string(),
                "-bs-ast".to_string(),
                "-o".to_string(),
                ast_path.to_string(),
                file,
            ],
        ]
        .concat(),
    )
}

fn generate_ast(
    package: packages::Package,
    root_package: packages::Package,
    filename: &str,
    version: &str,
    bsc_path: &str,
    workspace_root: &Option<String>,
) -> Result<(String, Option<String>), String> {
    let build_path_abs = package.get_build_path();
    let (ast_path, parser_args) = parser_args(
        &package.bsconfig,
        &root_package.bsconfig,
        filename,
        version,
        workspace_root,
        &root_package.path,
    );

    /* Create .ast */
    if let Some(res_to_ast) = Some(
        Command::new(bsc_path)
            .current_dir(&build_path_abs)
            .args(parser_args)
            .output()
            .expect("Error converting .res to .ast"),
    ) {
        let stderr = std::str::from_utf8(&res_to_ast.stderr).expect("Expect StdErr to be non-null");
        if helpers::contains_ascii_characters(stderr) {
            if res_to_ast.status.success() {
                Ok((ast_path, Some(stderr.to_string())))
            } else {
                println!("err: {}", stderr.to_string());
                Err(stderr.to_string())
            }
        } else {
            Ok((ast_path, None))
        }
    } else {
        println!("Parsing file {}...", filename);
        Err(format!(
            "Could not find canonicalize_string_path for file {} in package {}",
            filename, package.name
        ))
    }
}

fn path_to_ast_extension(path: &Path) -> &str {
    let extension = path.extension().unwrap().to_str().unwrap();
    if helpers::is_interface_ast_file(extension) {
        ".iast"
    } else {
        ".ast"
    }
}

fn filter_ppx_flags(ppx_flags: &Option<Vec<OneOrMore<String>>>) -> Option<Vec<OneOrMore<String>>> {
    // get the environment variable "BISECT_ENABLE" if it exists set the filter to "bisect"
    let filter = match std::env::var("BISECT_ENABLE") {
        Ok(_) => None,
        Err(_) => Some("bisect"),
    };
    match ppx_flags {
        Some(flags) => Some(
            flags
                .iter()
                .filter(|flag| match (flag, filter) {
                    (bsconfig::OneOrMore::Single(str), Some(filter)) => !str.contains(filter),
                    (bsconfig::OneOrMore::Multiple(str), Some(filter)) => {
                        !str.first().unwrap().contains(filter)
                    }
                    _ => true,
                })
                .map(|x| x.to_owned())
                .collect::<Vec<OneOrMore<String>>>(),
        ),
        None => None,
    }
}
