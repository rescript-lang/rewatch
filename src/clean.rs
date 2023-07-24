use crate::bsconfig;
use crate::build;
use crate::build_types::*;
use crate::helpers;
use crate::helpers::get_mlmap_path;
use crate::package_tree;
use ahash::{AHashMap, AHashSet};
use rayon::prelude::*;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

pub fn get_res_path_from_ast(ast_file: &str) -> Option<String> {
    if let Ok(lines) = helpers::read_lines(ast_file.to_string()) {
        // we skip the first line with is some null characters
        // the following lines in the AST are the dependency modules
        // we stop when we hit a line that starts with a "/", this is the path of the file.
        // this is the point where the dependencies end and the actual AST starts
        for line in lines.skip(1) {
            match line {
                Ok(line) if line.trim_start().starts_with('/') => return Some(line),
                _ => (),
            }
        }
    }
    return None;
}

fn remove_ast(source_file: &str, package_name: &str, root_path: &str, is_root: bool) {
    let _ = std::fs::remove_file(helpers::get_compiler_asset(
        source_file,
        package_name,
        &package_tree::Namespace::NoNamespace,
        root_path,
        "ast",
        is_root,
    ));
}

fn remove_iast(source_file: &str, package_name: &str, root_path: &str, is_root: bool) {
    let _ = std::fs::remove_file(helpers::get_compiler_asset(
        source_file,
        package_name,
        &package_tree::Namespace::NoNamespace,
        root_path,
        "iast",
        is_root,
    ));
}

fn remove_mjs_file(source_file: &str, suffix: &bsconfig::Suffix) {
    let _ = std::fs::remove_file(helpers::change_extension(
        source_file,
        // suffix.to_string includes the ., so we need to remove it
        &suffix.to_string()[1..],
    ));
}

fn remove_compile_assets(
    source_file: &str,
    package_name: &str,
    namespace: &package_tree::Namespace,
    root_path: &str,
    is_root: bool,
) {
    // optimization
    // only issue cmti if htere is an interfacce file
    for extension in &["cmj", "cmi", "cmt", "cmti"] {
        let _ = std::fs::remove_file(helpers::get_compiler_asset(
            source_file,
            package_name,
            namespace,
            root_path,
            extension,
            is_root,
        ));
        let _ = std::fs::remove_file(helpers::get_bs_compiler_asset(
            source_file,
            package_name,
            namespace,
            root_path,
            extension,
            is_root,
        ));
    }
}

pub fn clean_mjs_files(build_state: &BuildState, project_root: &str) {
    // get all rescript file locations
    let rescript_file_locations = build_state
        .modules
        .values()
        .filter_map(|module| match &module.source_type {
            SourceType::SourceFile(source_file) => {
                let package = build_state.packages.get(&module.package_name).unwrap();
                let root_package = build_state
                    .packages
                    .get(&build_state.root_config_name)
                    .expect("Could not find root package");
                Some((
                    std::path::PathBuf::from(helpers::get_package_path(
                        &project_root,
                        &module.package_name,
                        package.is_root,
                    ))
                    .join(source_file.implementation.path.to_string())
                    .to_string_lossy()
                    .to_string(),
                    root_package
                        .bsconfig
                        .suffix
                        .to_owned()
                        .unwrap_or(bsconfig::Suffix::Mjs),
                ))
            }
            _ => None,
        })
        .collect::<Vec<(String, bsconfig::Suffix)>>();

    rescript_file_locations
        .par_iter()
        .for_each(|(rescript_file_location, suffix)| {
            remove_mjs_file(&rescript_file_location, &suffix)
        });
}

pub fn cleanup_previous_build(build_state: &mut BuildState) -> (usize, usize, AHashSet<String>) {
    let mut ast_modules: AHashMap<
        String,
        (
            String,
            String,
            package_tree::Namespace,
            SystemTime,
            String,
            bool,
            Option<bsconfig::Suffix>,
        ),
    > = AHashMap::new();
    let mut cmi_modules: AHashMap<String, SystemTime> = AHashMap::new();
    let mut ast_rescript_file_locations = AHashSet::new();

    let mut rescript_file_locations = build_state
        .modules
        .values()
        .filter_map(|module| match &module.source_type {
            SourceType::SourceFile(source_file) => {
                let package = build_state.packages.get(&module.package_name).unwrap();

                Some(
                    PathBuf::from(helpers::get_package_path(
                        &build_state.project_root,
                        &module.package_name,
                        package.is_root,
                    ))
                    .canonicalize()
                    .expect("Could not canonicalize")
                    .join(source_file.implementation.path.to_owned())
                    .to_string_lossy()
                    .to_string(),
                )
            }
            _ => None,
        })
        .collect::<AHashSet<String>>();

    rescript_file_locations.extend(
        build_state
            .modules
            .values()
            .filter_map(|module| {
                let package = build_state.packages.get(&module.package_name).unwrap();
                build::get_interface(module).as_ref().map(|interface| {
                    PathBuf::from(helpers::get_package_path(
                        &build_state.project_root,
                        &module.package_name,
                        package.is_root,
                    ))
                    .canonicalize()
                    .expect("Could not canonicalize")
                    .join(interface.path.to_owned())
                    .to_string_lossy()
                    .to_string()
                })
            })
            .collect::<AHashSet<String>>(),
    );

    // scan all ast files in all packages
    for package in build_state.packages.values() {
        let read_dir = fs::read_dir(std::path::Path::new(&helpers::get_build_path(
            &build_state.project_root,
            &package.name,
            package.is_root,
        )))
        .unwrap();

        for entry in read_dir {
            match entry {
                Ok(entry) => {
                    let path = entry.path();
                    let extension = path.extension().and_then(|e| e.to_str());
                    match extension {
                        Some(ext) => match ext {
                            "iast" | "ast" => {
                                let module_name = helpers::file_path_to_module_name(
                                    path.to_str().unwrap(),
                                    &package.namespace,
                                );

                                let ast_file_path = path.to_str().unwrap().to_owned();
                                let res_file_path = get_res_path_from_ast(&ast_file_path);
                                let root_package = build_state
                                    .packages
                                    .get(&build_state.root_config_name)
                                    .expect("Could not find root package");
                                match res_file_path {
                                    Some(res_file_path) => {
                                        let _ = ast_modules.insert(
                                            res_file_path.to_owned(),
                                            (
                                                module_name,
                                                package.name.to_owned(),
                                                package.namespace.to_owned(),
                                                entry.metadata().unwrap().modified().unwrap(),
                                                ast_file_path,
                                                package.is_root,
                                                root_package.bsconfig.suffix.to_owned(),
                                            ),
                                        );
                                        let _ = ast_rescript_file_locations.insert(res_file_path);
                                    }
                                    None => (),
                                }
                            }
                            "cmi" => {
                                let module_name = helpers::file_path_to_module_name(
                                    path.to_str().unwrap(),
                                    &package.namespace,
                                );
                                cmi_modules.insert(
                                    module_name,
                                    entry.metadata().unwrap().modified().unwrap(),
                                );
                            }
                            _ => {
                                // println!("other extension: {:?}", other);
                            }
                        },
                        None => (),
                    }
                }
                Err(_) => (),
            }
        }
    }

    // delete the .mjs file which appear in our previous compile assets
    // but does not exists anymore
    // delete the compiler assets for which modules we can't find a rescript file
    // location of rescript file is in the AST
    // delete the .mjs file for which we DO have a compiler asset, but don't have a
    // rescript file anymore (path is found in the .ast file)
    let diff = ast_rescript_file_locations
        .difference(&rescript_file_locations)
        .collect::<Vec<&String>>();

    let diff_len = diff.len();

    let deleted_interfaces = diff
        .par_iter()
        .map(|res_file_location| {
            let (
                module_name,
                package_name,
                package_namespace,
                _last_modified,
                ast_file_path,
                is_root,
                suffix,
            ) = ast_modules
                .get(&res_file_location.to_string())
                .expect("Could not find module name for ast file");
            remove_compile_assets(
                res_file_location,
                package_name,
                package_namespace,
                &build_state.project_root,
                *is_root,
            );
            remove_mjs_file(
                &res_file_location,
                &suffix.to_owned().unwrap_or(bsconfig::Suffix::Mjs),
            );
            remove_iast(
                res_file_location,
                package_name,
                &build_state.project_root,
                *is_root,
            );
            remove_ast(
                res_file_location,
                package_name,
                &build_state.project_root,
                *is_root,
            );
            match helpers::get_extension(ast_file_path).as_str() {
                "iast" => Some(module_name.to_owned()),
                "ast" => None,
                _ => None,
            }
        })
        .collect::<Vec<Option<String>>>()
        .iter()
        .filter_map(|module_name| module_name.to_owned())
        .collect::<AHashSet<String>>();

    ast_rescript_file_locations
        .intersection(&rescript_file_locations)
        .into_iter()
        .for_each(|res_file_location| {
            let (
                module_name,
                _package_name,
                _package_namespace,
                ast_last_modified,
                ast_file_path,
                _is_root,
                _suffix,
            ) = ast_modules
                .get(res_file_location)
                .expect("Could not find module name for ast file");
            let module = build_state
                .modules
                .get_mut(module_name)
                .expect("Could not find module for ast file");
            let full_module_name = module_name.to_owned();

            let compile_dirty = cmi_modules.get(&full_module_name);
            if let Some(compile_dirty) = compile_dirty {
                // println!("{} is not dirty", module_name);
                let (implementation_last_modified, interface_last_modified) = match &module
                    .source_type
                {
                    SourceType::MlMap(_) => (None, None),
                    SourceType::SourceFile(source_file) => {
                        let implementation_last_modified = source_file.implementation.last_modified;
                        let interface_last_modified = source_file
                            .interface
                            .as_ref()
                            .map(|interface| interface.last_modified);
                        (Some(implementation_last_modified), interface_last_modified)
                    }
                };
                let last_modified = match (implementation_last_modified, interface_last_modified) {
                    (Some(implementation_last_modified), Some(interface_last_modified)) => {
                        if implementation_last_modified > interface_last_modified {
                            Some(implementation_last_modified)
                        } else {
                            Some(interface_last_modified)
                        }
                    }
                    (Some(implementation_last_modified), None) => {
                        Some(implementation_last_modified)
                    }
                    _ => None,
                };

                if let Some(last_modified) = last_modified {
                    if compile_dirty > &last_modified
                        && !deleted_interfaces.contains(&full_module_name)
                    {
                        module.compile_dirty = false;
                    }
                }
            }

            match &mut module.source_type {
                SourceType::MlMap(_) => unreachable!("MlMap is not matched with a ReScript file"),
                SourceType::SourceFile(source_file) => {
                    if helpers::is_interface_ast_file(ast_file_path) {
                        let interface = source_file
                            .interface
                            .as_mut()
                            .expect("Could not find interface for module");

                        let source_last_modified = interface.last_modified;
                        if ast_last_modified > &source_last_modified {
                            interface.dirty = false;
                        }
                    } else {
                        let implementation = &mut source_file.implementation;
                        let source_last_modified = implementation.last_modified;
                        if ast_last_modified > &source_last_modified
                            && !deleted_interfaces.contains(module_name)
                        {
                            implementation.dirty = false;
                        }
                    }
                }
            }
        });

    let ast_module_names = ast_modules
        .values()
        .filter_map(|(module_name, _, _, _, ast_file_path, _, _)| {
            match helpers::get_extension(ast_file_path).as_str() {
                "iast" => None,
                "ast" => Some(module_name),
                _ => None,
            }
        })
        .collect::<AHashSet<&String>>();

    let all_module_names = build_state
        .modules
        .keys()
        .map(|module_name| module_name)
        .collect::<AHashSet<&String>>();

    let deleted_module_names = ast_module_names
        .difference(&all_module_names)
        .map(|module_name| {
            // if the module is a namespace, we need to mark the whole namespace as dirty when a module has been deleted
            if let Some(namespace) = helpers::get_namespace_from_module_name(module_name) {
                return namespace;
            }
            return module_name.to_string();
        })
        .collect::<AHashSet<String>>();

    (
        diff_len,
        ast_rescript_file_locations.len(),
        deleted_module_names,
    )
}

fn failed_to_parse(module: &Module) -> bool {
    match &module.source_type {
        SourceType::SourceFile(SourceFile {
            implementation:
                Implementation {
                    parse_state: ParseState::ParseError | ParseState::Warning,
                    ..
                },
            ..
        }) => true,
        SourceType::SourceFile(SourceFile {
            interface:
                Some(Interface {
                    parse_state: ParseState::ParseError | ParseState::Warning,
                    ..
                }),
            ..
        }) => true,
        _ => false,
    }
}

fn failed_to_compile(module: &Module) -> bool {
    match &module.source_type {
        SourceType::SourceFile(SourceFile {
            implementation:
                Implementation {
                    compile_state: CompileState::Error | CompileState::Warning,
                    ..
                },
            ..
        }) => true,
        SourceType::SourceFile(SourceFile {
            interface:
                Some(Interface {
                    compile_state: CompileState::Error | CompileState::Warning,
                    ..
                }),
            ..
        }) => true,
        _ => false,
    }
}

pub fn cleanup_after_build(build_state: &BuildState) {
    build_state
        .modules
        .par_iter()
        .for_each(|(_module_name, module)| {
            let package = build_state.get_package(&module.package_name).unwrap();
            if failed_to_parse(module) {
                match &module.source_type {
                    SourceType::SourceFile(source_file) => {
                        remove_iast(
                            &source_file.implementation.path,
                            &module.package_name,
                            &build_state.project_root,
                            package.is_root,
                        );
                        remove_ast(
                            &source_file.implementation.path,
                            &module.package_name,
                            &build_state.project_root,
                            package.is_root,
                        );
                    }
                    _ => (),
                }
            }
            if failed_to_compile(module) {
                // only retain ast file if it compiled successfully, that's the only thing we check
                // if we see a AST file, we assume it compiled successfully, so we also need to clean
                // up the AST file if compile is not successful
                match &module.source_type {
                    SourceType::SourceFile(source_file) => {
                        remove_compile_assets(
                            &source_file.implementation.path,
                            &module.package_name,
                            &package.namespace,
                            &build_state.project_root,
                            package.is_root,
                        );
                    }
                    SourceType::MlMap(_) => remove_compile_assets(
                        &get_mlmap_path(
                            &build_state.project_root,
                            &module.package_name,
                            &package.namespace.to_suffix().unwrap(),
                            package.is_root,
                        ),
                        &module.package_name,
                        &package_tree::Namespace::NoNamespace,
                        &build_state.project_root,
                        package.is_root,
                    ),
                }
            }
        });
}
