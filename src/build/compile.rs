mod dependency_cycle;

use super::build_types::*;
use super::logs;
use super::packages;
use crate::bsconfig;
use crate::helpers;
use ahash::{AHashMap, AHashSet};
use console::style;
use log::debug;
use log::{info, log_enabled, Level::Info};
use rayon::prelude::*;
use std::path::Path;
use std::process::Command;

pub fn compile(
    mut build_state: &mut BuildState,
    deleted_module_names: &AHashSet<String>,
    rescript_version: &str,
    inc: impl Fn() -> () + std::marker::Sync,
    set_length: impl Fn(u64) -> (),
    bsc_path: &str,
) -> (String, String, usize) {
    let mut compiled_modules = AHashSet::<String>::new();

    mark_modules_with_deleted_deps_dirty(&mut build_state, &deleted_module_names);
    mark_modules_with_expired_deps_dirty(&mut build_state);

    let dirty_modules = build_state
        .modules
        .iter()
        .filter_map(|(module_name, module)| {
            if module.compile_dirty {
                Some(module_name.to_owned())
            } else {
                None
            }
        })
        .collect::<AHashSet<String>>();

    // dirty_modules.iter().for_each(|m| println!("dirty module: {}", m));
    // println!("{} dirty modules", dirty_modules.len());
    // let mut sorted_dirty_modules = dirty_modules.iter().collect::<Vec<&String>>();
    // sorted_dirty_modules.sort();
    // // dirty_modules.iter().for_each(|m| println!("dirty module: {}", m));
    // sorted_dirty_modules
    //     .iter()
    //     .for_each(|m| println!("dirty module: {}", m));

    // for sure clean modules -- after checking the hash of the cmi
    let mut clean_modules = AHashSet::<String>::new();

    // TODO: calculate the real dirty modules from the orginal dirty modules in each iteration
    // taken into account the modules that we know are clean, so they don't propagate through the
    // deps graph
    // create a hashset of all clean modules form the file-hashes
    let mut loop_count = 0;
    let mut files_total_count = compiled_modules.len();
    let mut files_current_loop_count;
    let mut compile_errors = "".to_string();
    let mut compile_warnings = "".to_string();
    let mut num_compiled_modules = 0;
    let mut sorted_modules = build_state.module_names.iter().collect::<Vec<&String>>();
    sorted_modules.sort();

    // this is the whole "compile universe" all modules that might be dirty
    // we get this by traversing from the dirty modules to all the modules that
    // are dependent on them
    let mut compile_universe = dirty_modules.clone();
    let mut current_step_modules = compile_universe.clone();
    loop {
        let mut dependents: AHashSet<String> = AHashSet::new();
        for dirty_module in current_step_modules.iter() {
            dependents.extend(build_state.get_module(dirty_module).unwrap().dependents.clone());
        }
        current_step_modules = dependents
            .difference(&compile_universe)
            .map(|s| s.to_string())
            .collect::<AHashSet<String>>();

        compile_universe.extend(current_step_modules.to_owned());
        if current_step_modules.is_empty() {
            break;
        }
    }

    let compile_universe_count = compile_universe.len();
    set_length(compile_universe_count as u64);

    // start off with all modules that have no deps in this compile universe
    let mut in_progress_modules = compile_universe
        .iter()
        .filter(|module_name| {
            let module = build_state.get_module(*module_name).unwrap();
            module.deps.intersection(&compile_universe).count() == 0
        })
        .map(|module_name| module_name.to_string())
        .collect::<AHashSet<String>>();

    loop {
        files_current_loop_count = 0;
        loop_count += 1;

        info!(
            "Compiled: {} out of {}. Compile loop: {}",
            files_total_count,
            compile_universe.len(),
            loop_count,
        );

        let current_in_progres_modules = in_progress_modules.clone();

        current_in_progres_modules
            .par_iter()
            .map(|module_name| {
                let module = build_state.get_module(module_name).unwrap();
                let package = build_state
                    .get_package(&module.package_name)
                    .expect("Package not found");
                // all dependencies that we care about are compiled
                if module
                    .deps
                    .intersection(&compile_universe)
                    .all(|dep| compiled_modules.contains(dep))
                {
                    if !module.compile_dirty {
                        // we are sure we don't have to compile this, so we can mark it as compiled and clean
                        return Some((module_name.to_string(), Ok(None), Some(Ok(None)), true, false));
                    }
                    match module.source_type.to_owned() {
                        SourceType::MlMap(_) => {
                            // the mlmap needs to be compiled before the files are compiled
                            // in the same namespace, otherwise we get a compile error
                            // this is why mlmap is compiled in the AST generation stage
                            // compile_mlmap(&module.package, module_name, &project_root);
                            Some((
                                package.namespace.to_suffix().unwrap(),
                                Ok(None),
                                Some(Ok(None)),
                                false,
                                false,
                            ))
                        }
                        SourceType::SourceFile(source_file) => {
                            let cmi_path = helpers::get_compiler_asset(
                                package,
                                &package.namespace,
                                &source_file.implementation.path,
                                "cmi",
                            );

                            let cmi_digest = helpers::compute_file_hash(&cmi_path);

                            let package = build_state
                                .get_package(&module.package_name)
                                .expect("Package not found");

                            let root_package =
                                build_state.get_package(&build_state.root_config_name).unwrap();

                            let interface_result = match source_file.interface.to_owned() {
                                Some(Interface { path, .. }) => {
                                    let result = compile_file(
                                        &package,
                                        &root_package,
                                        &package.get_iast_path(&path),
                                        module,
                                        &rescript_version,
                                        true,
                                        bsc_path,
                                        &build_state.packages,
                                    );
                                    Some(result)
                                }
                                _ => None,
                            };
                            let result = compile_file(
                                &package,
                                &root_package,
                                &package.get_ast_path(&source_file.implementation.path),
                                module,
                                &rescript_version,
                                false,
                                bsc_path,
                                &build_state.packages,
                            );
                            // if let Err(error) = result.to_owned() {
                            //     println!("{}", error);
                            //     panic!("Implementation compilation error!");
                            // }
                            let cmi_digest_after = helpers::compute_file_hash(&cmi_path);

                            // println!(
                            //     "cmi path {}, digest: {:?} / {:?}",
                            //     cmi_path, cmi_digest, cmi_digest_after
                            // );

                            // we want to compare both the hash of interface and the implementation
                            // compile assets to verify that nothing changed. We also need to checke the interface
                            // because we can include MyModule, so the modules that depend on this module might
                            // change when this modules interface does not change, but the implementation does
                            let is_clean_cmi = match (cmi_digest, cmi_digest_after) {
                                (Some(cmi_digest), Some(cmi_digest_after)) => {
                                    cmi_digest.eq(&cmi_digest_after)
                                }

                                _ => false,
                            };

                            Some((
                                module_name.to_string(),
                                result,
                                interface_result,
                                is_clean_cmi,
                                true,
                            ))
                        }
                    }
                } else {
                    None
                }
                .map(|res| {
                    if !(log_enabled!(Info)) {
                        inc();
                    }
                    res
                })
            })
            .collect::<Vec<
                Option<(
                    String,
                    Result<Option<String>, String>,
                    Option<Result<Option<String>, String>>,
                    bool,
                    bool,
                )>,
            >>()
            .iter()
            .for_each(|result| match result {
                Some((module_name, result, interface_result, is_clean, is_compiled)) => {
                    in_progress_modules.remove(module_name);

                    if *is_compiled {
                        num_compiled_modules += 1;
                    }

                    files_current_loop_count += 1;
                    compiled_modules.insert(module_name.to_string());

                    if *is_clean {
                        // actually add it to a list of clean modules
                        clean_modules.insert(module_name.to_string());
                    }

                    let module_dependents = build_state.get_module(module_name).unwrap().dependents.clone();

                    // if not clean -- compile modules that depend on this module
                    for dep in module_dependents.iter() {
                        //  mark the reverse dep as dirty when the source is not clean
                        if !*is_clean {
                            let dep_module = build_state.modules.get_mut(dep).unwrap();
                            //  mark the reverse dep as dirty when the source is not clean
                            dep_module.compile_dirty = true;
                        }
                        if !compiled_modules.contains(dep) {
                            in_progress_modules.insert(dep.to_string());
                        }
                    }

                    let module = build_state.modules.get_mut(module_name).unwrap();
                    let package = build_state
                        .packages
                        .get(&module.package_name)
                        .expect("Package not found");
                    match module.source_type {
                        SourceType::MlMap(_) => (),
                        SourceType::SourceFile(ref mut source_file) => {
                            match result {
                                Ok(Some(err)) => {
                                    source_file.implementation.compile_state = CompileState::Warning;
                                    logs::append(package, &err);
                                    compile_warnings.push_str(&err);
                                }
                                Ok(None) => (),
                                Err(err) => {
                                    source_file.implementation.compile_state = CompileState::Error;
                                    logs::append(package, &err);
                                    compile_errors.push_str(&err);
                                }
                            };
                            match interface_result {
                                Some(Ok(Some(err))) => {
                                    source_file.interface.as_mut().unwrap().compile_state =
                                        CompileState::Warning;
                                    logs::append(package, &err);
                                    compile_warnings.push_str(&err);
                                }
                                Some(Ok(None)) => (),
                                Some(Err(err)) => {
                                    source_file.interface.as_mut().unwrap().compile_state =
                                        CompileState::Error;
                                    logs::append(package, &err);
                                    compile_errors.push_str(&err);
                                }
                                _ => (),
                            };
                        }
                    }
                }
                None => (),
            });

        files_total_count += files_current_loop_count;

        if files_total_count == compile_universe_count {
            break;
        }
        if in_progress_modules.len() == 0 || in_progress_modules.eq(&current_in_progres_modules) {
            // find the dependency cycle
            let cycle = dependency_cycle::find(
                &compile_universe
                    .iter()
                    .map(|s| (s, build_state.get_module(s).unwrap()))
                    .collect::<Vec<(&String, &Module)>>(),
            );
            compile_errors.push_str(&format!(
                "\n{}\n{}\n",
                style("Can't continue... Found a circular dependency in your code:").red(),
                dependency_cycle::format(&cycle)
            ))
        }
        if compile_errors.len() > 0 {
            break;
        };
    }

    (compile_errors, compile_warnings, num_compiled_modules)
}

pub fn compiler_args(
    package: &packages::Package,
    root_package: &packages::Package,
    ast_path: &str,
    version: &str,
    file_path: &str,
    is_interface: bool,
    has_interface: bool,
    packages: &AHashMap<String, packages::Package>,
) -> Vec<String> {
    let normal_deps = package
        .bsconfig
        .bs_dependencies
        .as_ref()
        .unwrap_or(&vec![])
        .to_owned();

    let bsc_flags = bsconfig::flatten_flags(&package.bsconfig.bsc_flags);
    // don't compile dev-deps yet
    // let dev_deps = source
    //     .package
    //     .bsconfig
    //     .bs_dev_dependencies
    //     .as_ref()
    //     .unwrap_or(&vec![])
    //     .to_owned();

    let deps = vec![normal_deps]
        .concat()
        .into_iter()
        .map(|x| {
            let package = &packages.get(&x).expect("expect package");
            vec![
                "-I".to_string(),
                helpers::canonicalize_string_path(&package.get_build_path()).unwrap(),
            ]
        })
        .collect::<Vec<Vec<String>>>();

    let module_name = helpers::file_path_to_module_name(file_path, &package.namespace);

    let namespace_args = match &package.namespace {
        packages::Namespace::NamespaceWithEntry { namespace: _, entry } if &module_name == entry => {
            // if the module is the entry we just want to open the namespace
            vec![
                "-open".to_string(),
                package.namespace.to_suffix().unwrap().to_string(),
            ]
        }
        packages::Namespace::Namespace(_)
        | packages::Namespace::NamespaceWithEntry {
            namespace: _,
            entry: _,
        } => {
            vec![
                "-bs-ns".to_string(),
                package.namespace.to_suffix().unwrap().to_string(),
            ]
        }
        packages::Namespace::NoNamespace => vec![],
    };

    let jsx_args = root_package.get_jsx_args();
    let jsx_module_args = root_package.get_jsx_module_args();
    let jsx_mode_args = root_package.get_jsx_mode_args();
    let uncurried_args = package.get_uncurried_args(version, &root_package);

    let warning_args: Vec<String> = match package.bsconfig.warnings.to_owned() {
        None => vec![],
        Some(warnings) => {
            let warn_number = match warnings.number {
                None => vec![],
                Some(warnings) => {
                    vec!["-w".to_string(), warnings.to_string()]
                }
            };

            let warn_error = match warnings.error {
                Some(bsconfig::Error::Catchall(true)) => {
                    vec!["-warn-error".to_string(), "A".to_string()]
                }
                Some(bsconfig::Error::Qualified(errors)) => {
                    vec!["-warn-error".to_string(), errors.to_string()]
                }
                _ => vec![],
            };

            vec![warn_number, warn_error].concat()
        }
    };

    let read_cmi_args = match has_interface {
        true => {
            if is_interface {
                vec![]
            } else {
                vec!["-bs-read-cmi".to_string()]
            }
        }
        false => vec![],
    };

    let implementation_args = if is_interface {
        debug!("Compiling interface file: {}", &module_name);
        vec![]
    } else {
        debug!("Compiling file: {}", &module_name);

        // TODO: Also read suffix from package-spec.
        let suffix = match root_package.bsconfig.suffix.to_owned() {
            Some(suffix) => suffix,
            None => String::from(bsconfig::DEFAULT_SUFFIX),
        };

        vec![
            "-bs-package-name".to_string(),
            package.bsconfig.name.to_owned(),
            "-bs-package-output".to_string(),
            format!(
                "es6:{}:{}",
                Path::new(file_path).parent().unwrap().to_str().unwrap(),
                suffix
            ),
        ]
    };

    let to_mjs_args = vec![
        namespace_args,
        read_cmi_args,
        vec!["-I".to_string(), ".".to_string()],
        deps.concat(),
        jsx_args,
        jsx_module_args,
        jsx_mode_args,
        uncurried_args,
        bsc_flags.to_owned(),
        warning_args,
        // vec!["-warn-error".to_string(), "A".to_string()],
        // ^^ this one fails for bisect-ppx
        // this is the default
        // we should probably parse the right ones from the package config
        // vec!["-w".to_string(), "a".to_string()],
        implementation_args,
        // vec![
        //     "-I".to_string(),
        //     abs_node_modules_path.to_string() + "/rescript/ocaml",
        // ],
        vec![ast_path.to_string()],
    ]
    .concat();

    to_mjs_args
}

fn compile_file(
    package: &packages::Package,
    root_package: &packages::Package,
    ast_path: &str,
    module: &Module,
    version: &str,
    is_interface: bool,
    bsc_path: &str,
    packages: &AHashMap<String, packages::Package>,
) -> Result<Option<String>, String> {
    let build_path_abs = package.get_build_path();
    let implementation_file_path = match module.source_type {
        SourceType::SourceFile(ref source_file) => &source_file.implementation.path,
        _ => panic!("Not a source file"),
    };
    let module_name = helpers::file_path_to_module_name(implementation_file_path, &package.namespace);
    let has_interface = module.get_interface().is_some();
    let to_mjs_args = compiler_args(
        package,
        root_package,
        ast_path,
        version,
        &implementation_file_path,
        is_interface,
        has_interface,
        packages,
    );

    let to_mjs = Command::new(bsc_path)
        .current_dir(helpers::canonicalize_string_path(&build_path_abs.to_owned()).unwrap())
        .args(to_mjs_args)
        .output();

    match to_mjs {
        Ok(x) if !x.status.success() => {
            let stderr = String::from_utf8_lossy(&x.stderr);
            let stdout = String::from_utf8_lossy(&x.stdout);
            Err(stderr.to_string() + &stdout)
        }
        Err(e) => Err(format!("ERROR, {}, {:?}", e, ast_path)),
        Ok(x) => {
            let err = std::str::from_utf8(&x.stderr)
                .expect("stdout should be non-null")
                .to_string();

            let dir = std::path::Path::new(implementation_file_path).parent().unwrap();

            // perhaps we can do this copying somewhere else
            if !is_interface {
                let _ = std::fs::copy(
                    build_path_abs.to_string() + "/" + &module_name + ".cmi",
                    std::path::Path::new(&package.get_bs_build_path())
                        .join(dir)
                        // because editor tooling doesn't support namespace entries yet
                        // we just remove the @ for now. This makes sure the editor support
                        // doesn't break
                        .join(module_name.to_owned().replace("@", "") + ".cmi"),
                );
                let _ = std::fs::copy(
                    build_path_abs.to_string() + "/" + &module_name + ".cmj",
                    std::path::Path::new(&package.get_bs_build_path())
                        .join(dir)
                        .join(module_name.to_owned() + ".cmj"),
                );
                let _ = std::fs::copy(
                    build_path_abs.to_string() + "/" + &module_name + ".cmt",
                    std::path::Path::new(&package.get_bs_build_path())
                        .join(dir)
                        // because editor tooling doesn't support namespace entries yet
                        // we just remove the @ for now. This makes sure the editor support
                        // doesn't break
                        .join(module_name.to_owned().replace("@", "") + ".cmt"),
                );
            } else {
                let _ = std::fs::copy(
                    build_path_abs.to_string() + "/" + &module_name + ".cmti",
                    std::path::Path::new(&package.get_bs_build_path())
                        .join(dir)
                        .join(module_name.to_owned() + ".cmti"),
                );
            }
            match &module.source_type {
                SourceType::SourceFile(SourceFile {
                    interface: Some(Interface { path, .. }),
                    ..
                })
                | SourceType::SourceFile(SourceFile {
                    implementation: Implementation { path, .. },
                    ..
                }) => {
                    // we need to copy the source file to the build directory.
                    // editor tools expects the source file in lib/bs for finding the current package
                    // and in lib/ocaml when referencing modules in other packages
                    let _ = std::fs::copy(
                        std::path::Path::new(&package.path).join(path),
                        std::path::Path::new(&package.get_bs_build_path()).join(path),
                    )
                    .expect("copying source file failed");

                    let _ = std::fs::copy(
                        std::path::Path::new(&package.path).join(path),
                        std::path::Path::new(&package.get_build_path())
                            .join(std::path::Path::new(path).file_name().unwrap()),
                    )
                    .expect("copying source file failed");
                }
                _ => (),
            }

            if helpers::contains_ascii_characters(&err) {
                if package.is_pinned_dep {
                    // supress warnings of external deps
                    Ok(Some(err))
                } else {
                    Ok(None)
                }
            } else {
                Ok(None)
            }
        }
    }
}

pub fn mark_modules_with_deleted_deps_dirty(
    build_state: &mut BuildState,
    deleted_modules: &AHashSet<String>,
) {
    build_state.modules.iter_mut().for_each(|(_, module)| {
        if !module.deps.is_disjoint(deleted_modules) {
            module.compile_dirty = true;
        }
    });
}

pub fn mark_modules_with_expired_deps_dirty(build_state: &mut BuildState) {
    let mut modules_with_expired_deps: AHashSet<String> = AHashSet::new();
    build_state
        .modules
        .iter()
        .filter(|m| !m.1.is_mlmap())
        .for_each(|(module_name, module)| {
            for dependent in module.dependents.iter() {
                let dependent_module = build_state.modules.get(dependent).unwrap();
                match dependent_module.source_type {
                    SourceType::SourceFile(_) => {
                        match (module.last_compiled_cmt, module.last_compiled_cmi) {
                            (None, None) | (Some(_), None) | (None, Some(_)) => {
                                modules_with_expired_deps.insert(module_name.to_string());
                            }
                            (Some(_), Some(_)) => (),
                        }

                        // we compare the last compiled time of the dependent module with the last
                        // compile of the interface of the module it depends on, if the interface
                        // didn't change it doesn't matter
                        match (dependent_module.last_compiled_cmt, module.last_compiled_cmi) {
                            (Some(last_compiled_dependent), Some(last_compiled)) => {
                                if last_compiled_dependent < last_compiled {
                                    // println!(
                                    //     "✅ {} is a dependent of {} ({:?} / {:?})",
                                    //     module_name, dependent, last_compiled_dependent, last_compiled
                                    // );

                                    modules_with_expired_deps.insert(dependent.to_string());
                                } else {
                                    // println!(
                                    //     "🛑 {} is a dependent of {} ({:?} / {:?})",
                                    //     module_name, dependent, last_compiled_dependent, last_compiled
                                    // );
                                }
                            }
                            (None, _) => {
                                modules_with_expired_deps.insert(dependent.to_string());
                            }
                            _ => (),
                        }
                    }
                    // a namespace is never a dependent of a module (it can be a dependency, but not the other
                    // way around)
                    SourceType::MlMap(_) => {
                        for dependent_of_namespace in dependent_module.dependents.iter() {
                            let dependent_module = build_state.modules.get(dependent_of_namespace).unwrap();
                            match (dependent_module.last_compiled_cmt, module.last_compiled_cmi) {
                                (Some(last_compiled_dependent), Some(last_compiled)) => {
                                    if last_compiled_dependent < last_compiled {
                                        modules_with_expired_deps.insert(dependent.to_string());
                                    }
                                }
                                _ => (),
                            }
                        }
                    }
                }
            }
        });
    build_state.modules.iter_mut().for_each(|(module_name, module)| {
        if modules_with_expired_deps.contains(module_name) {
            module.compile_dirty = true;
        }
    });
}
