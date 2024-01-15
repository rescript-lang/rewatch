use super::build_types::*;
use super::packages;
use crate::helpers;
use ahash::{AHashMap, AHashSet};
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

pub fn read(build_state: &mut BuildState) -> CompileAssetsState {
    let mut ast_modules: AHashMap<String, AstModule> = AHashMap::new();
    let mut cmi_modules: AHashMap<String, SystemTime> = AHashMap::new();
    let mut cmt_modules: AHashMap<String, SystemTime> = AHashMap::new();
    let mut ast_rescript_file_locations = AHashSet::new();

    let mut rescript_file_locations = build_state
        .modules
        .values()
        .filter_map(|module| match &module.source_type {
            SourceType::SourceFile(source_file) => {
                let package = build_state.packages.get(&module.package_name).unwrap();

                Some(
                    PathBuf::from(&package.path)
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
                module.get_interface().as_ref().map(|interface| {
                    PathBuf::from(&package.path)
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
                                            AstModule {
                                                module_name: module_name,
                                                package_name: package.name.to_owned(),
                                                namespace: package.namespace.to_owned(),
                                                last_modified: entry.metadata().unwrap().modified().unwrap(),
                                                ast_file_path: ast_file_path,
                                                is_root: package.is_root,
                                                suffix: root_package.bsconfig.suffix.to_owned(),
                                            },
                                        );
                                        let _ = ast_rescript_file_locations.insert(res_file_path);
                                    }
                                    None => (),
                                }
                            }
                            "cmi" => {
                                let module_name = helpers::file_path_to_module_name(
                                    path.to_str().unwrap(),
                                    // we don't want to include a namespace here because the CMI file
                                    // already includes a namespace
                                    &packages::Namespace::NoNamespace,
                                );
                                cmi_modules
                                    .insert(module_name, entry.metadata().unwrap().modified().unwrap());
                            }
                            "cmt" => {
                                let module_name = helpers::file_path_to_module_name(
                                    path.to_str().unwrap(),
                                    // we don't want to include a namespace here because the CMI file
                                    // already includes a namespace
                                    &packages::Namespace::NoNamespace,
                                );
                                cmt_modules
                                    .insert(module_name, entry.metadata().unwrap().modified().unwrap());
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

    CompileAssetsState {
        ast_modules,
        cmi_modules,
        cmt_modules,
        ast_rescript_file_locations,
        rescript_file_locations,
    }
}

fn get_res_path_from_ast(ast_file: &str) -> Option<String> {
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
