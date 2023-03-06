use crate::bsconfig;
use crate::bsconfig::OneOrMore;
use crate::build_types::*;
use crate::clean;
use crate::helpers;
use crate::helpers::emojis::*;
use crate::package_tree;
use ahash::{AHashMap, AHashSet};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, error, info, log_enabled, Level::Info};
use rayon::prelude::*;
use std::fs;
use std::io::{stdout, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

pub fn get_res_path_from_ast(ast_file: &str) -> Option<String> {
    if let Ok(lines) = helpers::read_lines(ast_file.to_string()) {
        // we skip the first line with is some null characters
        // the following lines in the AST are the dependency modules
        // we stop when we hit a line that starts with a "/", this is the path of the file.
        // this is the point where the dependencies end and the actual AST starts
        for line in lines.skip(1) {
            match line {
                Ok(line) => {
                    let line = line.trim().to_string();
                    if line.starts_with('/') {
                        return Some(line);
                    }
                }
                Err(_) => (),
            }
        }
    }
    return None;
}

pub fn get_interface<'a>(module: &'a Module) -> &'a Option<Interface> {
    match &module.source_type {
        SourceType::SourceFile(source_file) => &source_file.interface,
        _ => &None,
    }
}

pub fn get_version(project_root: &str) -> String {
    let version_cmd = Command::new(helpers::get_bsc(&project_root))
        .args(["-v"])
        .output()
        .expect("failed to find version");

    std::str::from_utf8(&version_cmd.stdout)
        .expect("Could not read version from rescript")
        .replace("\n", "")
        .replace("ReScript ", "")
}

fn filter_ppx_flags(ppx_flags: &Option<Vec<OneOrMore<String>>>) -> Option<Vec<OneOrMore<String>>> {
    let filter = "bisect";
    match ppx_flags {
        Some(flags) => Some(
            flags
                .iter()
                .filter(|flag| match flag {
                    bsconfig::OneOrMore::Single(str) => !str.contains(filter),
                    bsconfig::OneOrMore::Multiple(str) => !str.first().unwrap().contains(filter),
                })
                .map(|x| x.to_owned())
                .collect::<Vec<OneOrMore<String>>>(),
        ),
        None => None,
    }
}

fn generate_ast(
    package: package_tree::Package,
    filename: &str,
    root_path: &str,
    version: &str,
) -> Result<(String, Option<String>), String> {
    let file = &filename.to_string();
    let build_path_abs = helpers::get_build_path(root_path, &package.name);
    let ast_extension = match PathBuf::from(filename)
        .extension()
        .unwrap()
        .to_str()
        .unwrap()
    {
        "resi" | "rei" | "mli" => ".iast",
        _ => ".ast",
    };

    let ast_path = (helpers::get_basename(&file.to_string()).to_owned()) + ast_extension;
    let abs_node_modules_path = helpers::get_node_modules_path(root_path);

    let ppx_flags = bsconfig::flatten_ppx_flags(
        &abs_node_modules_path,
        &filter_ppx_flags(&package.bsconfig.ppx_flags),
        &package.name,
    );

    let bsc_flags = bsconfig::flatten_flags(&package.bsconfig.bsc_flags);

    let res_to_ast_args = vec![
        vec!["-bs-v".to_string(), format!("{}", version)],
        ppx_flags,
        {
            package
                .bsconfig
                .reason
                .to_owned()
                .map(|x| vec!["-bs-jsx".to_string(), format!("{}", x.react_jsx)])
                .unwrap_or(vec![])
        },
        bsc_flags,
        vec![
            "-absname".to_string(),
            "-bs-ast".to_string(),
            "-o".to_string(),
            ast_path.to_string(),
            file.to_string(),
        ],
    ]
    .concat();

    /* Create .ast */
    let res_to_ast = Command::new(helpers::get_bsc(&root_path))
        .current_dir(build_path_abs.to_string())
        .args(res_to_ast_args)
        .output()
        .expect("Error converting .res to .ast");

    let stderr = std::str::from_utf8(&res_to_ast.stderr).expect("stderr should be non-null");

    if helpers::contains_ascii_characters(stderr) {
        if res_to_ast.status.success() {
            Ok((ast_path, Some(stderr.to_string())))
        } else {
            Err(stderr.to_string())
        }
    } else {
        Ok((ast_path, None))
    }
}

// Namespaces work like the following: The build system will generate a file
// called `MyModule.mlmap` which contains all modules that are in the namespace
//
// Not sure what the first line of this file is, but the next lines are names of
// the modules in the namespace you can call bsc with this file, and it will
// produce compiler assets for this file basically a module with all aliases.
// Given that this is just aliases, it doesn not need to create a mjs file.
//
// Internal modules are not accessible with the following trick, they are
// compiled to a module name such as `MyModule-MyNameSpace`.  A dash in a module
// name is not possible to make in a source file, but it's possible when
// constructing the AST, so these modules are hidden from compilation.
// in the top namespace however, we alias with the proper names

fn get_dep_modules(
    ast_file: &str,
    namespace: Option<String>,
    package_modules: &AHashSet<String>,
    valid_modules: &AHashSet<String>,
) -> AHashSet<String> {
    let mut deps = Vec::new();
    if let Ok(lines) = helpers::read_lines(ast_file.to_string()) {
        // we skip the first line with is some null characters
        // the following lines in the AST are the dependency modules
        // we stop when we hit a line that starts with a "/", this is the path of the file.
        // this is the point where the dependencies end and the actual AST starts
        for line in lines.skip(1) {
            match line {
                Ok(line) => {
                    let line = line.trim().to_string();
                    if line.starts_with('/') {
                        break;
                    } else if !line.is_empty() {
                        deps.push(line);
                    }
                }
                Err(_) => (),
            }
        }
    }

    return deps
        .into_iter()
        .map(|dep| {
            dep.split('.')
                .collect::<Vec<&str>>()
                .first()
                .unwrap()
                .to_string()
        })
        .map(|dep| match namespace.to_owned() {
            Some(namespace) => {
                let namespaced_name = dep.to_owned() + "-" + &namespace;
                if package_modules.contains(&namespaced_name) {
                    return namespaced_name;
                } else {
                    return dep;
                };
            }
            None => dep,
        })
        .filter(|dep| valid_modules.contains(dep))
        .filter(|dep| match namespace.to_owned() {
            Some(namespace) => !dep.eq(&namespace),
            None => true,
        })
        .collect::<AHashSet<String>>();
}

fn gen_mlmap(
    package: &package_tree::Package,
    namespace: &str,
    modules: &Vec<String>,
    root_path: &str,
) -> String {
    let build_path_abs = helpers::get_build_path(root_path, &package.name);
    // we don't really need to create a digest, because we track if we need to
    // recompile in a different way but we need to put it in the file for it to
    // be readable.
    let mut sorted_modules = modules.clone();
    sorted_modules.sort();
    let digest = "randjbuildsystem".to_owned() + "\n" + &sorted_modules.join("\n");
    let file = build_path_abs.to_string() + "/" + namespace + ".mlmap";
    fs::write(&file, digest).expect("Unable to write mlmap");
    file.to_string()
}

pub fn generate_asts<'a>(
    version: &str,
    project_root: &str,
    modules: &'a mut AHashMap<String, Module>,
    all_modules: &AHashSet<String>,
    deleted_modules: &AHashSet<String>,
) -> Result<String, String> {
    let mut has_failure = false;
    let mut stderr = "".to_string();

    let results = modules
        .par_iter()
        .map(|(module_name, module)| {
            debug!("Generating AST for module: {}", module_name);
            match &module.source_type {
                SourceType::MlMap(_mlmap) => {
                    compile_mlmap(&module.package, module_name, &project_root);

                    (
                        module_name.to_owned(),
                        Ok((
                            helpers::get_mlmap_path(
                                &project_root,
                                &module.package.name,
                                &module
                                    .package
                                    .namespace
                                    .as_ref()
                                    .expect("namespace should be set for mlmap module"),
                            ),
                            None,
                        )),
                        Ok(None),
                        module.deps.to_owned(),
                        None,
                    )
                }

                SourceType::SourceFile(source_file) => {
                    let (ast_path, asti_path) = if source_file.implementation.dirty
                        || source_file
                            .interface
                            .as_ref()
                            .map(|i| i.dirty)
                            .unwrap_or(false)
                    {
                        let ast_result = generate_ast(
                            module.package.to_owned(),
                            &source_file.implementation.path.to_owned(),
                            &project_root,
                            &version,
                        );

                        let asti_result =
                            match source_file.interface.as_ref().map(|i| i.path.to_owned()) {
                                Some(interface_file_path) => generate_ast(
                                    module.package.to_owned(),
                                    &interface_file_path.to_owned(),
                                    &project_root,
                                    &version,
                                )
                                .map(|result| Some(result)),
                                _ => Ok(None),
                            };

                        (ast_result, asti_result)
                    } else {
                        (
                            Ok((
                                helpers::get_basename(&source_file.implementation.path).to_string()
                                    + ".ast",
                                // helpers::get_ast_path(
                                //     &source_file.implementation.path,
                                //     &module.package.name,
                                //     &project_root,
                                // ),
                                None,
                            )),
                            Ok(source_file.interface.as_ref().map(|i| {
                                (
                                    // helpers::get_iast_path(
                                    //     &i.path,
                                    //     &module.package.name,
                                    //     &project_root,
                                    // )
                                    helpers::get_basename(&i.path).to_string() + ".iast",
                                    None,
                                )
                            })),
                        )
                    };

                    let build_path =
                        helpers::get_build_path(project_root, &module.package.bsconfig.name);

                    // choose the namespaced dep if that module appears in the package, otherwise global dep
                    let deps = if let (Ok((ast_path, _stderr)), Ok(asti_path)) =
                        (ast_path.to_owned(), asti_path.to_owned())
                    {
                        let mut deps = get_dep_modules(
                            &(build_path.to_string() + "/" + &ast_path),
                            module.package.namespace.to_owned(),
                            &module.package.modules.as_ref().unwrap(),
                            &all_modules.union(deleted_modules).cloned().collect(),
                        );

                        match asti_path.to_owned() {
                            Some((asti_path, _stderr)) => deps.extend(get_dep_modules(
                                &(build_path.to_owned() + "/" + &asti_path),
                                module.package.namespace.to_owned(),
                                &module.package.modules.as_ref().unwrap(),
                                &all_modules.union(deleted_modules).cloned().collect(),
                            )),
                            None => (),
                        }

                        deps.remove(module_name);
                        deps
                    } else {
                        AHashSet::new()
                    };

                    (
                        module_name.to_owned(),
                        ast_path,
                        asti_path,
                        deps,
                        module.package.namespace.to_owned(),
                    )
                }
            }
        })
        .collect::<Vec<(
            String,
            Result<(String, Option<String>), String>,
            Result<Option<(String, Option<String>)>, String>,
            AHashSet<String>,
            Option<String>,
        )>>();

    let mut dirty_modules = deleted_modules.clone();
    dirty_modules.extend(
        modules
            .iter()
            .filter_map(|(module_name, module)| {
                if is_dirty(&module) {
                    Some(module_name.to_string())
                } else {
                    None
                }
            })
            .collect::<AHashSet<String>>(),
    );

    results
        .into_iter()
        .for_each(|(module_name, ast_path, iast_path, deps, _namespace)| {
            if let Some(module) = modules.get_mut(&module_name) {
                module.deps = deps;
                match ast_path {
                    Ok((_path, err)) => {
                        // supress warnings in non-pinned deps
                        if module.package.is_pinned_dep {
                            if let Some(err) = err {
                                match module.source_type {
                                    SourceType::SourceFile(ref mut source_file) => {
                                        source_file.implementation.parse_state =
                                            ParseState::Warning;
                                    }
                                    _ => (),
                                }
                                stderr.push_str(&err);
                            }
                        }
                    }
                    Err(err) => {
                        match module.source_type {
                            SourceType::SourceFile(ref mut source_file) => {
                                source_file.implementation.parse_state = ParseState::ParseError;
                            }
                            _ => (),
                        }
                        has_failure = true;
                        stderr.push_str(&err);
                    }
                };
                match iast_path {
                    Ok(Some((_path, err))) => {
                        // supress warnings in non-pinned deps
                        if module.package.is_pinned_dep {
                            if let Some(err) = err {
                                match module.source_type {
                                    SourceType::SourceFile(ref mut source_file) => {
                                        source_file.interface.as_mut().map(|interface| {
                                            interface.parse_state = ParseState::ParseError
                                        });
                                    }
                                    _ => (),
                                }
                                stderr.push_str(&err);
                            }
                        }
                    }
                    Ok(None) => (),
                    Err(err) => {
                        match module.source_type {
                            SourceType::SourceFile(ref mut source_file) => {
                                source_file.interface.as_mut().map(|interface| {
                                    interface.parse_state = ParseState::ParseError
                                });
                            }
                            _ => (),
                        }
                        has_failure = true;
                        stderr.push_str(&err);
                    }
                };

                // if dirty_modules.contains(&module_name) {
                //     match module.source_type {
                //         SourceType::SourceFile(ref mut source_file) => {
                //             source_file.implementation.dirty = true;
                //             match source_file.interface {
                //                 Some(ref mut interface) => {
                //                     interface.dirty = true;
                //                 }
                //                 None => (),
                //             }
                //         }
                //         SourceType::MlMap(ref mut mlmap) => {
                //             mlmap.dirty = true;
                //         }
                //     }
                // }
            }
        });

    if has_failure {
        Err(stderr)
    } else {
        Ok(stderr)
    }
}

pub fn parse_packages(
    project_root: &str,
    packages: AHashMap<String, package_tree::Package>,
) -> (AHashSet<String>, AHashMap<String, Module>) {
    let mut modules: AHashMap<String, Module> = AHashMap::new();
    let mut all_modules: AHashSet<String> = AHashSet::new();

    packages.iter().for_each(|(package_name, package)| {
        debug!("Parsing package: {}", package_name);
        match package.modules.to_owned() {
            Some(package_modules) => all_modules.extend(package_modules),
            None => (),
        }
        let build_path_abs = helpers::get_build_path(project_root, &package.bsconfig.name);
        helpers::create_build_path(&build_path_abs);

        package.namespace.iter().for_each(|namespace| {
            // generate the mlmap "AST" file for modules that have a namespace configured
            let source_files = match package.source_files.to_owned() {
                Some(source_files) => source_files
                    .keys()
                    .map(|key| key.to_owned())
                    .collect::<Vec<String>>(),
                None => unreachable!(),
            };

            let depending_modules = source_files
                .iter()
                .map(|path| helpers::file_path_to_module_name(&path, &None))
                .collect::<AHashSet<String>>();

            let mlmap = gen_mlmap(
                &package,
                namespace,
                &Vec::from_iter(depending_modules.to_owned()),
                project_root,
            );

            // mlmap will be compiled in the AST generation step
            // compile_mlmap(&package, namespace, &project_root);

            let deps = source_files
                .iter()
                .map(|path| helpers::file_path_to_module_name(&path, &package.namespace))
                .collect::<AHashSet<String>>();

            modules.insert(
                helpers::file_path_to_module_name(&mlmap.to_owned(), &None),
                Module {
                    source_type: SourceType::MlMap(MlMap { dirty: false }),
                    deps: deps,
                    package: package.to_owned(),
                    compile_dirty: false,
                },
            );
        });

        debug!("Building source file-tree for package: {}", package.name);
        match &package.source_files {
            None => (),
            Some(source_files) => source_files.iter().for_each(|(file, metadata)| {
                let namespace = package.namespace.to_owned();

                let file_buf = PathBuf::from(file);
                let extension = file_buf.extension().unwrap().to_str().unwrap();
                let is_implementation = match extension {
                    "res" | "ml" | "re" => true,
                    _ => false,
                };
                let module_name = helpers::file_path_to_module_name(&file.to_owned(), &namespace);

                if is_implementation {
                    modules
                        .entry(module_name.to_string())
                        .and_modify(|module| match module.source_type {
                            SourceType::SourceFile(ref mut source_file) => {
                                if source_file.implementation.path.len() > 0 {
                                    error!("Duplicate files found for module: {}", &module_name);
                                    error!("file 1: {}", &source_file.implementation.path);
                                    error!("file 2: {}", &file);

                                    panic!("Unable to continue... See log output above...");
                                }
                                source_file.implementation.path = file.to_owned();
                                source_file.implementation.last_modified =
                                    metadata.modified().unwrap();
                                source_file.implementation.dirty = true;
                            }
                            _ => (),
                        })
                        .or_insert(Module {
                            source_type: SourceType::SourceFile(SourceFile {
                                implementation: Implementation {
                                    path: file.to_owned(),
                                    parse_state: ParseState::Pending,
                                    compile_state: CompileState::Pending,
                                    last_modified: metadata.modified().unwrap(),
                                    dirty: true,
                                },
                                interface: None,
                            }),
                            deps: AHashSet::new(),
                            package: package.to_owned(),
                            compile_dirty: true,
                        });
                } else {
                    modules
                        .entry(module_name.to_string())
                        .and_modify(|module| match module.source_type {
                            SourceType::SourceFile(ref mut source_file) => {
                                source_file.interface = Some(Interface {
                                    path: file.to_owned(),
                                    parse_state: ParseState::Pending,
                                    compile_state: CompileState::Pending,
                                    last_modified: metadata.modified().unwrap(),
                                    dirty: true,
                                });
                            }
                            _ => (),
                        })
                        .or_insert(Module {
                            source_type: SourceType::SourceFile(SourceFile {
                                // this will be overwritten later
                                implementation: Implementation {
                                    path: "".to_string(),
                                    parse_state: ParseState::Pending,
                                    compile_state: CompileState::Pending,
                                    last_modified: metadata.modified().unwrap(),
                                    dirty: false,
                                },
                                interface: Some(Interface {
                                    path: file.to_owned(),
                                    parse_state: ParseState::Pending,
                                    compile_state: CompileState::Pending,
                                    last_modified: metadata.modified().unwrap(),
                                    dirty: true,
                                }),
                            }),
                            deps: AHashSet::new(),
                            package: package.to_owned(),
                            compile_dirty: true,
                        });
                }
            }),
        }
    });

    (all_modules, modules)
}

pub fn compile_mlmap(package: &package_tree::Package, namespace: &str, root_path: &str) {
    let build_path_abs = helpers::get_build_path(root_path, &package.name);
    let mlmap_name = format!("{}.mlmap", namespace);
    let args = vec![vec![
        "-w",
        "-49",
        "-color",
        "always",
        "-no-alias-deps",
        &mlmap_name,
    ]]
    .concat();

    let _ = Command::new(helpers::get_bsc(&root_path))
        .current_dir(build_path_abs.to_string())
        .args(args)
        .output()
        .expect("err");
}

pub fn compile_file(
    package_name: &str,
    ast_path: &str,
    module: &Module,
    root_path: &str,
    is_interface: bool,
) -> Result<Option<String>, String> {
    let build_path_abs = helpers::get_build_path(root_path, package_name);
    let pkg_path_abs = helpers::get_package_path(root_path, package_name);
    let bsc_flags = bsconfig::flatten_flags(&module.package.bsconfig.bsc_flags);

    let normal_deps = module
        .package
        .bsconfig
        .bs_dependencies
        .as_ref()
        .unwrap_or(&vec![])
        .to_owned();

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
        .map(|x| vec!["-I".to_string(), helpers::get_build_path(root_path, &x)])
        .collect::<Vec<Vec<String>>>();

    let namespace_args = match module.package.namespace.to_owned() {
        Some(namespace) => vec!["-bs-ns".to_string(), namespace],
        None => vec![],
    };

    let read_cmi_args = match get_interface(module) {
        Some(_) => {
            if is_interface {
                vec![]
            } else {
                vec!["-bs-read-cmi".to_string()]
            }
        }
        _ => vec![],
    };

    let implementation_file_path = match module.source_type {
        SourceType::SourceFile(ref source_file) => &source_file.implementation.path,
        _ => panic!("Not a source file"),
    };

    let module_name =
        helpers::file_path_to_module_name(implementation_file_path, &module.package.namespace);

    let implementation_args = if is_interface {
        debug!("Compiling interface file: {}", &module_name);
        vec![]
    } else {
        debug!("Compiling file: {}", &module_name);
        vec![
            "-bs-package-name".to_string(),
            module.package.bsconfig.name.to_owned(),
            "-bs-package-output".to_string(),
            format!(
                "es6:{}:.mjs",
                Path::new(implementation_file_path)
                    .strip_prefix(pkg_path_abs)
                    .unwrap()
                    .parent()
                    .unwrap()
                    .to_str()
                    .unwrap(),
            ),
        ]
    };

    let to_mjs_args = vec![
        namespace_args,
        read_cmi_args,
        vec!["-I".to_string(), ".".to_string()],
        deps.concat(),
        bsc_flags,
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
        vec![ast_path.to_owned()],
    ]
    .concat();

    let to_mjs = Command::new(helpers::get_bsc(&root_path))
        .current_dir(build_path_abs.to_string())
        .args(to_mjs_args)
        .output();

    match to_mjs {
        Ok(x) if !x.status.success() => {
            Err(
                // "Problem compiling file: ".to_string()
                // + if !is_interface {
                //     &module.file_path
                // } else {
                //     &module.interface_file_path.as_ref().unwrap()
                // }
                // + "\n\n"
                // +
                std::str::from_utf8(&x.stderr)
                    .expect("stderr should be non-null")
                    .to_string(),
            )
        }
        Err(e) => Err(format!("ERROR, {}, {:?}", e, ast_path)),
        Ok(x) => {
            let err = std::str::from_utf8(&x.stderr)
                .expect("stdout should be non-null")
                .to_string();

            let dir = std::path::Path::new(implementation_file_path)
                .strip_prefix(helpers::get_package_path(root_path, &module.package.name))
                .unwrap()
                .parent()
                .unwrap();

            // perhaps we can do this copying somewhere else
            if !is_interface {
                let _ = std::fs::copy(
                    build_path_abs.to_string() + "/" + &module_name + ".cmi",
                    std::path::Path::new(&helpers::get_bs_build_path(
                        root_path,
                        &module.package.name,
                    ))
                    .join(dir)
                    .join(module_name.to_owned() + ".cmi"),
                );
                let _ = std::fs::copy(
                    build_path_abs.to_string() + "/" + &module_name + ".cmj",
                    std::path::Path::new(&helpers::get_bs_build_path(
                        root_path,
                        &module.package.name,
                    ))
                    .join(dir)
                    .join(module_name.to_owned() + ".cmj"),
                );
                let _ = std::fs::copy(
                    build_path_abs.to_string() + "/" + &module_name + ".cmt",
                    std::path::Path::new(&helpers::get_bs_build_path(
                        root_path,
                        &module.package.name,
                    ))
                    .join(dir)
                    .join(module_name.to_owned() + ".cmt"),
                );
            } else {
                let _ = std::fs::copy(
                    build_path_abs.to_string() + "/" + &module_name + ".cmti",
                    std::path::Path::new(&helpers::get_bs_build_path(
                        root_path,
                        &module.package.name,
                    ))
                    .join(dir)
                    .join(module_name.to_owned() + ".cmti"),
                );
            }

            if helpers::contains_ascii_characters(&err) {
                if module.package.is_pinned_dep {
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

pub fn clean(path: &str) {
    let project_root = helpers::get_abs_path(path);
    let packages = package_tree::make(&project_root);

    packages.iter().for_each(|(_, package)| {
        println!("Cleaning {}...", package.name);
        let path = std::path::Path::new(&package.package_dir)
            .join("lib")
            .join("ocaml");
        let _ = std::fs::remove_dir_all(path);
        let path = std::path::Path::new(&package.package_dir)
            .join("lib")
            .join("bs");
        let _ = std::fs::remove_dir_all(path);
    })
}

fn is_dirty(module: &Module) -> bool {
    match module.source_type {
        SourceType::SourceFile(SourceFile {
            implementation: Implementation { dirty: true, .. },
            ..
        }) => true,
        SourceType::SourceFile(SourceFile {
            interface: Some(Interface { dirty: true, .. }),
            ..
        }) => true,
        SourceType::SourceFile(_) => false,
        SourceType::MlMap(MlMap { dirty, .. }) => dirty,
    }
}

fn compute_file_hash(path: &str) -> Option<blake3::Hash> {
    match fs::read(path) {
        Ok(str) => Some(blake3::hash(&str)),
        Err(_) => None,
    }
}

pub fn build(path: &str) -> Result<AHashMap<std::string::String, Module>, ()> {
    let timing_total = Instant::now();
    let project_root = helpers::get_abs_path(path);
    let rescript_version = get_version(&project_root);

    print!(
        "{} {} Building package tree...",
        style("[1/5]").bold().dim(),
        TREE
    );
    let _ = stdout().flush();
    let timing_package_tree = Instant::now();
    let packages = package_tree::make(&project_root);
    let timing_package_tree_elapsed = timing_package_tree.elapsed();
    println!(
        "{}\r{} {}Built package tree in {:.2}s",
        LINE_CLEAR,
        style("[1/5]").bold().dim(),
        CHECKMARK,
        timing_package_tree_elapsed.as_secs_f64()
    );

    let timing_source_files = Instant::now();
    print!(
        "{} {} Finding source files...",
        style("[2/5]").bold().dim(),
        LOOKING_GLASS
    );
    let _ = stdout().flush();
    let (all_modules, mut modules) = parse_packages(&project_root, packages.to_owned());
    let timing_source_files_elapsed = timing_source_files.elapsed();
    println!(
        "{}\r{} {}Found source files in {:.2}s",
        LINE_CLEAR,
        style("[2/5]").bold().dim(),
        CHECKMARK,
        timing_source_files_elapsed.as_secs_f64()
    );

    print!(
        "{} {} Cleaning up previous build...",
        style("[3/5]").bold().dim(),
        SWEEP
    );
    let timing_cleanup = Instant::now();
    let (diff_cleanup, total_cleanup, deleted_module_names) =
        clean::cleanup_previous_build(&packages, &mut modules, &project_root);
    let timing_cleanup_elapsed = timing_cleanup.elapsed();
    println!(
        "{}\r{} {}Cleaned {}/{} {:.2}s",
        LINE_CLEAR,
        style("[3/5]").bold().dim(),
        CHECKMARK,
        diff_cleanup,
        total_cleanup,
        timing_cleanup_elapsed.as_secs_f64()
    );

    print!(
        "{} {} Parsing source files...",
        style("[4/5]").bold().dim(),
        CODE
    );
    let _ = stdout().flush();

    let timing_ast = Instant::now();
    let result_asts = generate_asts(
        &rescript_version,
        &project_root,
        &mut modules,
        &all_modules,
        &deleted_module_names,
    );
    let timing_ast_elapsed = timing_ast.elapsed();

    match result_asts {
        Ok(err) => {
            println!(
                "{}\r{} {}Parsed source files in {:.2}s",
                LINE_CLEAR,
                style("[4/5]").bold().dim(),
                CHECKMARK,
                timing_ast_elapsed.as_secs_f64()
            );
            println!("{}", &err);
        }
        Err(err) => {
            println!(
                "{}\r{} {}Error parsing source files in {:.2}s",
                LINE_CLEAR,
                style("[4/5]").bold().dim(),
                CROSS,
                timing_ast_elapsed.as_secs_f64()
            );
            println!("{}", &err);
            clean::cleanup_after_build(&modules, &AHashSet::new(), &all_modules, &project_root);
            return Err(());
        }
    }

    let pb = ProgressBar::new(modules.len().try_into().unwrap());
    pb.set_style(
        ProgressStyle::with_template(&format!(
            "{} {} Compiling... {{wide_bar}} {{pos}}/{{len}} {{msg}}",
            style("[5/5]").bold().dim(),
            SWORDS
        ))
        .unwrap(),
    );
    let start_compiling = Instant::now();

    let mut compiled_modules = AHashSet::<String>::new();
    // println!("Clean modules:");
    let dirty_modules = modules
        .iter()
        .filter_map(|(module_name, module)| {
            // if is_dirty(module) {
            if module.compile_dirty {
                // println!("> {}", module_name);
                Some(module_name.to_owned())
            } else {
                // println!("> {}", module_name);
                None
            }
        })
        .collect::<AHashSet<String>>();

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
    let total_modules = modules.len();
    let mut num_compiled_modules = 0;
    let mut sorted_modules = modules
        .iter()
        .map(|(module_name, _)| module_name.to_owned())
        .collect::<Vec<String>>();
    sorted_modules.sort();

    loop {
        if dirty_modules.len() == 0 {
            break;
        }
        let mut indirectly_dirty_modules = dirty_modules.clone();
        let mut checked_modules = AHashSet::with_capacity(modules.len());
        loop {
            let mut num_checked_modules = 0;
            for (module_name, module) in modules.iter() {
                if !checked_modules.contains(module_name) {
                    num_checked_modules += 1;
                    if module.deps.is_subset(&checked_modules) {
                        checked_modules.insert(module_name.to_string());
                        let is_dirty = module.deps.iter().any(|dep| {
                            if clean_modules.contains(dep) {
                                return false;
                            }
                            if indirectly_dirty_modules.contains(dep) {
                                return true;
                            }
                            return false;
                        });
                        // if !module.deps.is_disjoint(&indirectly_dirty_modules) {
                        //     indirectly_dirty_modules.insert(module_name.to_string());
                        // }
                        if is_dirty {
                            indirectly_dirty_modules.insert(module_name.to_string());
                        }
                    }
                }
            }
            if num_checked_modules == 0 {
                break;
            }
        }
        // let to_check_modules: Vec<(&String, &Module)> = modules
        //     .iter()
        //     .filter(|(module_name, _)| !checked_modules.contains(*module_name))
        //     .collect();
        // to_check_modules.iter().for_each(|(module_name, _)| {
        //     let _ = checked_modules.insert(module_name.to_string());
        // });
        // Parallel version -- much faster in a debug build but slower in a release build...
        // to_check_modules
        //     .par_iter()
        //     .map(|(module_name, module)| {
        //         if module.deps.is_subset(&checked_modules) {
        //             // checked_modules.insert(module_name.to_string());
        //             let is_dirty = module.deps.iter().any(|dep| {
        //                 if clean_modules.contains(dep) {
        //                     return false;
        //                 }
        //                 if indirectly_dirty_modules.contains(dep) {
        //                     return true;
        //                 }
        //                 return false;
        //             });

        //             if is_dirty {
        //                 return Some(module_name);
        //             }
        //         }
        //         return None;
        //     })
        //     .filter_map(|x| x)
        //     .collect::<Vec<&&String>>()
        //     .iter()
        //     .for_each(|module_name| {
        //         indirectly_dirty_modules.insert(module_name.to_string());
        //     });
        // if to_check_modules.len() == 0 {
        //     break;
        // }

        files_current_loop_count = 0;
        loop_count += 1;

        info!(
            "Compiled: {} out of {}. Compile loop: {}",
            files_total_count,
            modules.len(),
            loop_count,
        );

        sorted_modules
            // .iter()
            .par_iter()
            .map(|module_name| {
                let module = modules.get(module_name).unwrap();
                if module.deps.is_subset(&compiled_modules)
                    && !compiled_modules.contains(module_name)
                {
                    if !indirectly_dirty_modules.contains(module_name) {
                        // we are sure we don't have to compile this, so we can mark it as compiled
                        return Some((
                            module_name.to_string(),
                            Ok(None),
                            Some(Ok(None)),
                            true,
                            false,
                        ));
                    }

                    match module.source_type.to_owned() {
                        SourceType::MlMap(_) => {
                            // the mlmap needs to be compiled before the files are compiled
                            // in the same namespace, otherwise we get a compile error
                            // this is why mlmap is compiled in the AST generation stage
                            // compile_mlmap(&module.package, module_name, &project_root);
                            Some((
                                module.package.namespace.to_owned().unwrap(),
                                Ok(None),
                                Some(Ok(None)),
                                false,
                                false,
                            ))
                        }
                        SourceType::SourceFile(source_file) => {
                            let cmi_path = helpers::get_compiler_asset(
                                &source_file.implementation.path,
                                &module.package.name,
                                &module.package.namespace,
                                &project_root,
                                "cmi",
                            );
                            let cmi_digest = compute_file_hash(&cmi_path);

                            let interface_result = match source_file.interface.to_owned() {
                                Some(Interface { path, .. }) => {
                                    let result = compile_file(
                                        &module.package.name,
                                        &helpers::get_iast_path(
                                            &path,
                                            &module.package.name,
                                            &project_root,
                                        ),
                                        module,
                                        &project_root,
                                        true,
                                    );
                                    Some(result)
                                }
                                _ => None,
                            };
                            // if let Some(Err(error)) = interface_result.to_owned() {
                            //     println!("{}", error);
                            //     panic!("Interface compilation error!");
                            // }
                            let result = compile_file(
                                &module.package.name,
                                &helpers::get_ast_path(
                                    &source_file.implementation.path,
                                    &module.package.name,
                                    &project_root,
                                ),
                                module,
                                &project_root,
                                false,
                            );
                            // if let Err(error) = result.to_owned() {
                            //     println!("{}", error);
                            //     panic!("Implementation compilation error!");
                            // }
                            let cmi_digest_after = compute_file_hash(&cmi_path);

                            let is_clean = match (cmi_digest, cmi_digest_after) {
                                (Some(cmi_digest), Some(cmi_digest_after)) => {
                                    if cmi_digest.eq(&cmi_digest_after) {
                                        // println!(
                                        //     "{} is clean -- {:x} {:x}",
                                        //     cmi_path, cmi_digest, cmi_digest_after
                                        // );
                                        true
                                    } else {
                                        // println!(
                                        //     "{} is dirty -- {:x} {:x}",
                                        //     cmi_path, cmi_digest, cmi_digest_after
                                        // );
                                        false
                                    }
                                }

                                _ => false,
                            };
                            Some((
                                module_name.to_string(),
                                result,
                                interface_result,
                                is_clean,
                                true,
                            ))
                        }
                    }
                } else {
                    None
                }
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
                    if !(log_enabled!(Info)) {
                        pb.inc(1);
                    }
                    if *is_compiled {
                        num_compiled_modules += 1;
                    }
                    files_current_loop_count += 1;
                    compiled_modules.insert(module_name.to_string());

                    if *is_clean {
                        // actually add it to a list of clean modules
                        clean_modules.insert(module_name.to_string());
                    }

                    let module = modules.get_mut(module_name).unwrap();

                    match module.source_type {
                        SourceType::MlMap(_) => (),
                        SourceType::SourceFile(ref mut source_file) => {
                            match result {
                                Ok(Some(err)) => {
                                    source_file.implementation.compile_state =
                                        CompileState::Warning;
                                    compile_warnings.push_str(&err);
                                }
                                Ok(None) => (),
                                Err(err) => {
                                    source_file.implementation.compile_state = CompileState::Error;
                                    compile_errors.push_str(&err);
                                }
                            };
                            match interface_result {
                                Some(Ok(Some(err))) => {
                                    source_file.interface.as_mut().unwrap().compile_state =
                                        CompileState::Warning;
                                    compile_warnings.push_str(&err);
                                }
                                Some(Ok(None)) => (),
                                Some(Err(err)) => {
                                    source_file.interface.as_mut().unwrap().compile_state =
                                        CompileState::Error;
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

        if files_total_count == total_modules {
            break;
        }
        if files_current_loop_count == 0 {
            // we probably want to find the cycle(s), and give a helpful error message here
            compile_errors.push_str("Can't continue... Dependency cycle\n")
        }
        if compile_errors.len() > 0 {
            break;
        };
    }
    let compile_duration = start_compiling.elapsed();

    pb.finish();
    clean::cleanup_after_build(&modules, &compiled_modules, &all_modules, &project_root);
    if compile_errors.len() > 0 {
        println!(
            "{}\r{} {}Compiled {} modules in {:.2}s",
            LINE_CLEAR,
            style("[5/5]").bold().dim(),
            CROSS,
            num_compiled_modules,
            compile_duration.as_secs_f64()
        );
        if helpers::contains_ascii_characters(&compile_warnings) {
            println!("{}", &compile_warnings);
        }
        println!("{}", &compile_errors);
        return Err(());
    } else {
        println!(
            "{}\r{} {}Compiled {} modules in {:.2}s",
            LINE_CLEAR,
            style("[5/5]").bold().dim(),
            CHECKMARK,
            num_compiled_modules,
            compile_duration.as_secs_f64()
        );
        if helpers::contains_ascii_characters(&compile_warnings) {
            println!("{}", &compile_warnings);
        }
    }

    let timing_total_elapsed = timing_total.elapsed();
    println!("Done in {:.2}s", timing_total_elapsed.as_secs_f64());

    Ok(modules)
}
