use crate::bsconfig;
use crate::bsconfig::OneOrMore;
use crate::build_types::*;
use crate::clean;
use crate::clean::clean_mjs_files;
use crate::helpers;
use crate::helpers::emojis::*;
use crate::logs;
use crate::package_tree;
use ahash::AHashSet;
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, error, info, log_enabled, Level::Info};
use rayon::prelude::*;
use std::fs::{self, File};
use std::io::{stdout, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

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

fn get_jsx_args(package: &package_tree::Package) -> Vec<String> {
    match (
        package.bsconfig.reason.to_owned(),
        package.bsconfig.jsx.to_owned(),
    ) {
        (_, Some(jsx)) => match jsx.version {
            Some(version) if version == 3 || version == 4 => {
                vec!["-bs-jsx".to_string(), version.to_string()]
            }
            Some(_version) => panic!("Unsupported JSX version"),
            None => vec![],
        },
        (Some(reason), None) => {
            vec!["-bs-jsx".to_string(), format!("{}", reason.react_jsx)]
        }
        _ => vec![],
    }
}

fn get_jsx_mode_args(package: &package_tree::Package) -> Vec<String> {
    match package.bsconfig.jsx.to_owned() {
        Some(jsx) => match jsx.mode {
            Some(bsconfig::JsxMode::Classic) => {
                vec!["-bs-jsx-mode".to_string(), "classic".to_string()]
            }
            Some(bsconfig::JsxMode::Automatic) => {
                vec!["-bs-jsx-mode".to_string(), "automatic".to_string()]
            }

            None => vec![],
        },
        _ => vec![],
    }
}

fn get_jsx_module_args(package: &package_tree::Package) -> Vec<String> {
    match package.bsconfig.jsx.to_owned() {
        Some(jsx) => match jsx.module {
            Some(bsconfig::JsxModule::React) => {
                vec!["-bs-jsx-module".to_string(), "react".to_string()]
            }
            None => vec![],
        },
        _ => vec![],
    }
}

fn check_if_rescript11_or_higher(version: &str) -> bool {
    version.split(".").nth(0).unwrap().parse::<usize>().unwrap() >= 11
}

fn get_uncurried_args(
    version: &str,
    package: &package_tree::Package,
    root_package: &package_tree::Package,
) -> Vec<String> {
    if check_if_rescript11_or_higher(version) {
        match (
            root_package.bsconfig.uncurried.to_owned(),
            package.bsconfig.uncurried.to_owned(),
        ) {
            (Some(x), _) | (None, Some(x)) => {
                if x {
                    vec!["-uncurried".to_string()]
                } else {
                    vec![]
                }
            }
            (None, None) => vec!["-uncurried".to_string()],
        }
    } else {
        vec![]
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

fn path_to_ast_extension(path: &Path) -> &str {
    let extension = path.extension().unwrap().to_str().unwrap();
    return if helpers::is_interface_ast_file(extension) {
        ".iast"
    } else {
        ".ast"
    };
}

fn generate_ast(
    package: package_tree::Package,
    root_package: package_tree::Package,
    filename: &str,
    root_path: &str,
    version: &str,
) -> Result<(String, Option<String>), String> {
    let file = &filename.to_string();
    let build_path_abs = helpers::get_build_path(root_path, &package.name, package.is_root);
    let path = PathBuf::from(filename);
    let ast_extension = path_to_ast_extension(&path);

    let ast_path = (helpers::get_basename(&file.to_string()).to_owned()) + ast_extension;
    let abs_node_modules_path = helpers::get_node_modules_path(root_path);

    let ppx_flags = bsconfig::flatten_ppx_flags(
        &abs_node_modules_path,
        &filter_ppx_flags(&package.bsconfig.ppx_flags),
        &package.name,
    );

    let jsx_args = get_jsx_args(&root_package);
    let jsx_module_args = get_jsx_module_args(&root_package);
    let jsx_mode_args = get_jsx_mode_args(&root_package);
    let uncurried_args = get_uncurried_args(version, &package, &root_package);
    let bsc_flags = bsconfig::flatten_flags(&package.bsconfig.bsc_flags);

    let res_to_ast_args = |file: &str| -> Vec<String> {
        let file = "../../".to_string() + file;
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
        .concat()
    };

    /* Create .ast */
    if let Some(res_to_ast) = Some(file).map(|file| {
        Command::new(helpers::get_bsc(&root_path))
            .current_dir(helpers::canonicalize_string_path(&build_path_abs).unwrap())
            .args(res_to_ast_args(file))
            .output()
            .expect("Error converting .res to .ast")
    }) {
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
        println!("Parsing file {}...", file);
        return Err(format!(
            "Could not find canonicalize_string_path for file {} in package {}",
            file, package.name
        ));
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
    let mut deps = AHashSet::new();
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
                        deps.insert(line);
                    }
                }
                Err(_) => (),
            }
        }
    } else {
        panic!("Could not read file {}", ast_file);
    }

    return deps
        .iter()
        .map(|dep| {
            let dep_first = dep.split('.').next().unwrap();
            let dep_second = dep.split('.').nth(1);
            match &namespace {
                Some(namespace) => {
                    // if the module is in the own namespace, take the submodule -- so:
                    // if the module is TeamwalnutApp.MyModule inside of the namespace TeamwalnutApp
                    // we need the dependency to be MyModule in the same namespace
                    let dep = match dep_second {
                        Some(dep_second) if dep_first == namespace => dep_second,
                        _ => dep_first,
                    };
                    let namespaced_name = dep.to_owned() + "-" + &namespace;
                    if package_modules.contains(&namespaced_name) {
                        return namespaced_name;
                    } else {
                        return dep.to_string();
                    };
                }
                None => dep_first.to_string(),
            }
        })
        .filter(|dep| {
            valid_modules.contains(dep)
                && match namespace.to_owned() {
                    Some(namespace) => !dep.eq(&namespace),
                    None => true,
                }
        })
        .collect::<AHashSet<String>>();
}

fn gen_mlmap(
    package: &package_tree::Package,
    namespace: &str,
    depending_modules: &AHashSet<String>,
    root_path: &str,
) -> String {
    let build_path_abs = helpers::get_build_path(root_path, &package.name, package.is_root);
    // we don't really need to create a digest, because we track if we need to
    // recompile in a different way but we need to put it in the file for it to
    // be readable.

    let path = build_path_abs.to_string() + "/" + namespace + ".mlmap";
    let mut file = File::create(&path).expect("Unable to create mlmap");

    file.write_all(b"randjbuildsystem\n" as &[u8])
        .expect("Unable to write mlmap");

    let mut modules = Vec::from_iter(depending_modules.to_owned());
    modules.sort();
    for module in modules {
        // check if the module names is referencible in code (no exotic module names)
        // (only contains A-Z a-z 0-9 and _ and only starts with a capital letter)
        // if not, it does not make sense to export as part of the name space
        // this helps compile times of exotic modules such as MyModule.test
        file.write_all(module.as_bytes()).unwrap();
        file.write_all(b"\n").unwrap();
    }

    path.to_string()
}

fn generate_asts(version: &str, build_state: &mut BuildState, pb: &ProgressBar) -> Result<String, String> {
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
                SourceType::MlMap(_) => {
                    // probably better to do this in a different function
                    // specific to compiling mlmaps
                    let path = helpers::get_mlmap_path(
                        &build_state.project_root,
                        &module.package_name,
                        &package
                            .namespace
                            .to_suffix()
                            .expect("namespace should be set for mlmap module"),
                        package.is_root,
                    );
                    let compile_path = helpers::get_mlmap_compile_path(
                        &build_state.project_root,
                        &module.package_name,
                        &package
                            .namespace
                            .to_suffix()
                            .expect("namespace should be set for mlmap module"),
                        package.is_root,
                    );
                    let mlmap_hash = compute_file_hash(&compile_path);
                    compile_mlmap(&package, module_name, &build_state.project_root);
                    let mlmap_hash_after = compute_file_hash(&compile_path);

                    let is_dirty = match (mlmap_hash, mlmap_hash_after) {
                        (Some(digest), Some(digest_after)) => !digest.eq(&digest_after),
                        _ => true,
                    };

                    (module_name.to_owned(), Ok((path, None)), Ok(None), is_dirty)
                }

                SourceType::SourceFile(source_file) => {
                    let root_package = build_state.get_package(&build_state.root_config_name).unwrap();

                    let (ast_path, iast_path) = if source_file.implementation.dirty
                        || source_file.interface.as_ref().map(|i| i.dirty).unwrap_or(false)
                    {
                        pb.inc(1);
                        let ast_result = generate_ast(
                            package.to_owned(),
                            root_package.to_owned(),
                            &source_file.implementation.path.to_owned(),
                            &build_state.project_root,
                            &version,
                        );

                        let iast_result = match source_file.interface.as_ref().map(|i| i.path.to_owned()) {
                            Some(interface_file_path) => generate_ast(
                                package.to_owned(),
                                root_package.to_owned(),
                                &interface_file_path.to_owned(),
                                &build_state.project_root,
                                &version,
                            )
                            .map(|result| Some(result)),
                            _ => Ok(None),
                        };

                        (ast_result, iast_result)
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
                        )
                    };

                    (module_name.to_owned(), ast_path, iast_path, true)
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
                    match module.source_type {
                        SourceType::MlMap(_) => module.compile_dirty = true,
                        _ => (),
                    }
                }
                match ast_path {
                    Ok((_path, err)) => {
                        // supress warnings in non-pinned deps
                        if package.is_pinned_dep {
                            if let Some(err) = err {
                                match module.source_type {
                                    SourceType::SourceFile(ref mut source_file) => {
                                        source_file.implementation.parse_state = ParseState::Warning;
                                    }
                                    _ => (),
                                }
                                logs::append(&build_state.project_root, package.is_root, &package.name, &err);
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
                        logs::append(&build_state.project_root, package.is_root, &package.name, &err);
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
                                        source_file
                                            .interface
                                            .as_mut()
                                            .map(|interface| interface.parse_state = ParseState::ParseError);
                                    }
                                    _ => (),
                                }
                                logs::append(&build_state.project_root, package.is_root, &package.name, &err);
                                stderr.push_str(&err);
                            }
                        }
                    }
                    Ok(None) => (),
                    Err(err) => {
                        match module.source_type {
                            SourceType::SourceFile(ref mut source_file) => {
                                source_file
                                    .interface
                                    .as_mut()
                                    .map(|interface| interface.parse_state = ParseState::ParseError);
                            }
                            _ => (),
                        }
                        logs::append(&build_state.project_root, package.is_root, &package.name, &err);
                        has_failure = true;
                        stderr.push_str(&err);
                    }
                };
            }
        });

    if has_failure {
        Err(stderr)
    } else {
        Ok(stderr)
    }
}

fn get_deps(build_state: &mut BuildState, deleted_modules: &AHashSet<String>) {
    let all_mod = &build_state.module_names.union(deleted_modules).cloned().collect();
    build_state
        .modules
        .par_iter()
        .map(|(module_name, module)| match &module.source_type {
            SourceType::MlMap(_) => (module_name.to_string(), module.deps.to_owned()),
            SourceType::SourceFile(source_file) => {
                let package = build_state
                    .get_package(&module.package_name)
                    .expect("Package not found");
                let ast_path = helpers::get_ast_path(
                    &source_file.implementation.path,
                    &module.package_name,
                    &build_state.project_root,
                    package.is_root,
                );

                let mut deps = get_dep_modules(
                    &ast_path,
                    package.namespace.to_suffix(),
                    &package.modules.as_ref().unwrap(),
                    all_mod,
                );

                match &source_file.interface {
                    Some(interface) => {
                        let iast_path = helpers::get_iast_path(
                            &interface.path,
                            &module.package_name,
                            &build_state.project_root,
                            package.is_root,
                        );

                        deps.extend(get_dep_modules(
                            &iast_path,
                            package.namespace.to_suffix(),
                            &package.modules.as_ref().unwrap(),
                            all_mod,
                        ))
                    }
                    None => (),
                }
                match &package.namespace {
                    package_tree::Namespace::NamespaceWithEntry { namespace: _, entry }
                        if entry == module_name =>
                    {
                        deps.insert(package.namespace.to_suffix().unwrap());
                    }
                    _ => (),
                }
                deps.remove(module_name);
                (module_name.to_string(), deps)
            }
        })
        .collect::<Vec<(String, AHashSet<String>)>>()
        .into_iter()
        .for_each(|(module_name, deps)| {
            if let Some(module) = build_state.modules.get_mut(&module_name) {
                module.deps = deps.clone();
            }
            deps.iter().for_each(|dep_name| {
                if let Some(module) = build_state.modules.get_mut(dep_name) {
                    module.reverse_deps.insert(module_name.to_string());
                }
            });
        });
}

pub fn parse_packages(build_state: &mut BuildState) {
    // let mut all_modules: AHashSet<String> = AHashSet::new();
    // let packages = &build_state.packages;
    // let modules = &mut build_state.modules;

    build_state
        .packages
        .clone()
        .iter()
        .for_each(|(package_name, package)| {
            debug!("Parsing package: {}", package_name);
            match package.modules.to_owned() {
                Some(package_modules) => build_state.module_names.extend(package_modules),
                None => (),
            }
            let build_path_abs =
                helpers::get_build_path(&build_state.project_root, &package.bsconfig.name, package.is_root);
            let bs_build_path = helpers::get_bs_build_path(
                &build_state.project_root,
                &package.bsconfig.name,
                package.is_root,
            );
            helpers::create_build_path(&build_path_abs);
            helpers::create_build_path(&bs_build_path);

            package.namespace.to_suffix().iter().for_each(|namespace| {
                // generate the mlmap "AST" file for modules that have a namespace configured
                let source_files = match package.source_files.to_owned() {
                    Some(source_files) => source_files
                        .keys()
                        .map(|key| key.to_owned())
                        .collect::<Vec<String>>(),
                    None => unreachable!(),
                };
                let entry = match &package.namespace {
                    package_tree::Namespace::NamespaceWithEntry { entry, namespace: _ } => Some(entry),
                    _ => None,
                };

                let depending_modules = source_files
                    .iter()
                    .map(|path| {
                        helpers::file_path_to_module_name(&path, &package_tree::Namespace::NoNamespace)
                    })
                    .filter(|module_name| {
                        if let Some(entry) = entry {
                            module_name != entry
                        } else {
                            true
                        }
                    })
                    .filter(|module_name| helpers::is_non_exotic_module_name(module_name))
                    .collect::<AHashSet<String>>();

                let mlmap = gen_mlmap(&package, namespace, &depending_modules, &build_state.project_root);

                // mlmap will be compiled in the AST generation step
                // compile_mlmap(&package, namespace, &project_root);
                let deps = source_files
                    .iter()
                    .filter(|path| {
                        helpers::is_non_exotic_module_name(&helpers::file_path_to_module_name(
                            &path,
                            &package_tree::Namespace::NoNamespace,
                        ))
                    })
                    .map(|path| helpers::file_path_to_module_name(&path, &package.namespace))
                    .filter(|module_name| {
                        if let Some(entry) = entry {
                            module_name != entry
                        } else {
                            true
                        }
                    })
                    .collect::<AHashSet<String>>();

                build_state.insert_module(
                    &helpers::file_path_to_module_name(
                        &mlmap.to_owned(),
                        &package_tree::Namespace::NoNamespace,
                    ),
                    Module {
                        source_type: SourceType::MlMap(MlMap { dirty: false }),
                        deps,
                        reverse_deps: AHashSet::new(),
                        package_name: package.name.to_owned(),
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
                    let module_name = helpers::file_path_to_module_name(&file.to_owned(), &namespace);

                    if helpers::is_implementation_file(extension) {
                        build_state
                            .modules
                            .entry(module_name.to_string())
                            .and_modify(|module| match module.source_type {
                                SourceType::SourceFile(ref mut source_file) => {
                                    if &source_file.implementation.path != file {
                                        error!("Duplicate files found for module: {}", &module_name);
                                        error!("file 1: {}", &source_file.implementation.path);
                                        error!("file 2: {}", &file);

                                        panic!("Unable to continue... See log output above...");
                                    }
                                    source_file.implementation.path = file.to_owned();
                                    source_file.implementation.last_modified = metadata.modified;
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
                                        last_modified: metadata.modified,
                                        dirty: true,
                                    },
                                    interface: None,
                                }),
                                deps: AHashSet::new(),
                                reverse_deps: AHashSet::new(),
                                package_name: package.name.to_owned(),
                                compile_dirty: true,
                            });
                    } else {
                        // remove last character of string: resi -> res, rei -> re, mli -> ml
                        let mut implementation_filename = file.to_owned();
                        implementation_filename.pop();
                        match source_files.get(&implementation_filename) {
                            None => {
                                println!(
                                "{}\rWarning: No implementation file found for interface file (skipping): {}",
                                LINE_CLEAR, file
                            )
                            }
                            Some(_) => {
                                build_state
                                    .modules
                                    .entry(module_name.to_string())
                                    .and_modify(|module| match module.source_type {
                                        SourceType::SourceFile(ref mut source_file) => {
                                            source_file.interface = Some(Interface {
                                                path: file.to_owned(),
                                                parse_state: ParseState::Pending,
                                                compile_state: CompileState::Pending,
                                                last_modified: metadata.modified,
                                                dirty: true,
                                            });
                                        }
                                        _ => (),
                                    })
                                    .or_insert(Module {
                                        source_type: SourceType::SourceFile(SourceFile {
                                            // this will be overwritten later
                                            implementation: Implementation {
                                                path: implementation_filename.to_string(),
                                                parse_state: ParseState::Pending,
                                                compile_state: CompileState::Pending,
                                                last_modified: metadata.modified,
                                                dirty: true,
                                            },
                                            interface: Some(Interface {
                                                path: file.to_owned(),
                                                parse_state: ParseState::Pending,
                                                compile_state: CompileState::Pending,
                                                last_modified: metadata.modified,
                                                dirty: true,
                                            }),
                                        }),
                                        deps: AHashSet::new(),
                                        reverse_deps: AHashSet::new(),
                                        package_name: package.name.to_owned(),
                                        compile_dirty: true,
                                    });
                            }
                        }
                    }
                }),
            }
        });
}

pub fn compile_mlmap(package: &package_tree::Package, namespace: &str, root_path: &str) {
    let build_path_abs = helpers::get_build_path(root_path, &package.name, package.is_root);
    let mlmap_name = format!("{}.mlmap", namespace);
    let args = vec!["-w", "-49", "-color", "always", "-no-alias-deps", &mlmap_name];

    let _ = Command::new(helpers::get_bsc(&root_path))
        .current_dir(helpers::canonicalize_string_path(&build_path_abs).unwrap())
        .args(args)
        .output()
        .expect("err");
}

pub fn compile_file(
    package: &package_tree::Package,
    root_package: &package_tree::Package,
    ast_path: &str,
    module: &Module,
    root_path: &str,
    version: &str,
    is_interface: bool,
) -> Result<Option<String>, String> {
    let build_path_abs = helpers::get_build_path(root_path, &package.name, package.is_root);
    let bsc_flags = bsconfig::flatten_flags(&package.bsconfig.bsc_flags);

    let normal_deps = package
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
        .map(|x| {
            vec![
                "-I".to_string(),
                helpers::canonicalize_string_path(&helpers::get_build_path(root_path, &x, package.is_root))
                    .unwrap(),
            ]
        })
        .collect::<Vec<Vec<String>>>();

    let implementation_file_path = match module.source_type {
        SourceType::SourceFile(ref source_file) => &source_file.implementation.path,
        _ => panic!("Not a source file"),
    };

    let module_name = helpers::file_path_to_module_name(implementation_file_path, &package.namespace);

    let namespace_args = match &package.namespace {
        package_tree::Namespace::NamespaceWithEntry { namespace: _, entry } if &module_name == entry => {
            // if the module is the entry we just want to open the namespace
            vec![
                "-open".to_string(),
                package.namespace.to_suffix().unwrap().to_string(),
            ]
        }
        package_tree::Namespace::Namespace(_)
        | package_tree::Namespace::NamespaceWithEntry {
            namespace: _,
            entry: _,
        } => {
            vec![
                "-bs-ns".to_string(),
                package.namespace.to_suffix().unwrap().to_string(),
            ]
        }
        package_tree::Namespace::NoNamespace => vec![],
    };

    let jsx_args = get_jsx_args(&root_package);
    let jsx_module_args = get_jsx_module_args(&root_package);
    let jsx_mode_args = get_jsx_mode_args(&root_package);
    let uncurried_args = get_uncurried_args(version, &package, &root_package);

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

    let implementation_args = if is_interface {
        debug!("Compiling interface file: {}", &module_name);
        vec![]
    } else {
        debug!("Compiling file: {}", &module_name);

        // TODO: Also read suffix from package-spec.
        let suffix = match root_package.bsconfig.suffix.to_owned() {
            Some(suffix) => suffix,
            None => bsconfig::Suffix::Mjs,
        };

        vec![
            "-bs-package-name".to_string(),
            package.bsconfig.name.to_owned(),
            "-bs-package-output".to_string(),
            format!(
                "es6:{}:{}",
                Path::new(implementation_file_path)
                    .parent()
                    .unwrap()
                    .to_str()
                    .unwrap(),
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
        bsc_flags,
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

    let to_mjs = Command::new(helpers::get_bsc(&root_path))
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
                    std::path::Path::new(&helpers::get_bs_build_path(
                        root_path,
                        &package.name,
                        package.is_root,
                    ))
                    .join(dir)
                    .join(module_name.to_owned() + ".cmi"),
                );
                let _ = std::fs::copy(
                    build_path_abs.to_string() + "/" + &module_name + ".cmj",
                    std::path::Path::new(&helpers::get_bs_build_path(
                        root_path,
                        &package.name,
                        package.is_root,
                    ))
                    .join(dir)
                    .join(module_name.to_owned() + ".cmj"),
                );
                let _ = std::fs::copy(
                    build_path_abs.to_string() + "/" + &module_name + ".cmt",
                    std::path::Path::new(&helpers::get_bs_build_path(
                        root_path,
                        &package.name,
                        package.is_root,
                    ))
                    .join(dir)
                    .join(module_name.to_owned() + ".cmt"),
                );
            } else {
                let _ = std::fs::copy(
                    build_path_abs.to_string() + "/" + &module_name + ".cmti",
                    std::path::Path::new(&helpers::get_bs_build_path(
                        root_path,
                        &package.name,
                        package.is_root,
                    ))
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
                        std::path::Path::new(&helpers::get_package_path(
                            root_path,
                            &package.name,
                            package.is_root,
                        ))
                        .join(path),
                        std::path::Path::new(&helpers::get_bs_build_path(
                            root_path,
                            &package.name,
                            package.is_root,
                        ))
                        .join(path),
                    )
                    .expect("copying source file failed");

                    let _ = std::fs::copy(
                        std::path::Path::new(&helpers::get_package_path(
                            root_path,
                            &package.name,
                            package.is_root,
                        ))
                        .join(path),
                        std::path::Path::new(&helpers::get_build_path(
                            root_path,
                            &package.name,
                            package.is_root,
                        ))
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

pub fn clean(path: &str) {
    let project_root = helpers::get_abs_path(path);
    let packages = package_tree::make(&None, &project_root);
    let root_config_name = package_tree::get_package_name(&project_root);

    let timing_clean_compiler_assets = Instant::now();
    print!(
        "{} {} Cleaning compiler assets...",
        style("[1/2]").bold().dim(),
        SWEEP
    );
    std::io::stdout().flush().unwrap();
    packages.iter().for_each(|(_, package)| {
        print!(
            "{}\r{} {} Cleaning {}...",
            LINE_CLEAR,
            style("[1/2]").bold().dim(),
            SWEEP,
            package.name
        );
        std::io::stdout().flush().unwrap();

        let path_str = helpers::get_build_path(&project_root, &package.name, package.is_root);
        let path = std::path::Path::new(&path_str);
        let _ = std::fs::remove_dir_all(path);

        let path_str = helpers::get_bs_build_path(&project_root, &package.name, package.is_root);
        let path = std::path::Path::new(&path_str);
        let _ = std::fs::remove_dir_all(path);
    });
    let timing_clean_compiler_assets_elapsed = timing_clean_compiler_assets.elapsed();

    println!(
        "{}\r{} {}Cleaned compiler assets in {:.2}s",
        LINE_CLEAR,
        style("[1/2]").bold().dim(),
        CHECKMARK,
        timing_clean_compiler_assets_elapsed.as_secs_f64()
    );
    std::io::stdout().flush().unwrap();

    let timing_clean_mjs = Instant::now();
    print!("{} {} Cleaning mjs files...", style("[2/2]").bold().dim(), SWEEP);
    std::io::stdout().flush().unwrap();
    let mut build_state = BuildState::new(project_root.to_owned(), root_config_name, packages);
    parse_packages(&mut build_state);
    clean_mjs_files(&build_state, &project_root);
    let timing_clean_mjs_elapsed = timing_clean_mjs.elapsed();
    println!(
        "{}\r{} {}Cleaned mjs files in {:.2}s",
        LINE_CLEAR,
        style("[2/2]").bold().dim(),
        CHECKMARK,
        timing_clean_mjs_elapsed.as_secs_f64()
    );
    std::io::stdout().flush().unwrap();
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
        SourceType::MlMap(MlMap { dirty, .. }) => module.compile_dirty || dirty,
    }
}

fn compute_file_hash(path: &str) -> Option<blake3::Hash> {
    match fs::read(path) {
        Ok(str) => Some(blake3::hash(&str)),
        Err(_) => None,
    }
}

pub fn build(filter: &Option<regex::Regex>, path: &str, no_timing: bool) -> Result<BuildState, ()> {
    let timing_total = Instant::now();
    let project_root = helpers::get_abs_path(path);
    let rescript_version = get_version(&project_root);
    let root_config_name = package_tree::get_package_name(&project_root);
    let default_timing: Option<std::time::Duration> = if no_timing {
        Some(std::time::Duration::new(0.0 as u64, 0.0 as u32))
    } else {
        None
    };

    print!(
        "{} {} Building package tree...",
        style("[1/7]").bold().dim(),
        TREE
    );
    let _ = stdout().flush();
    let timing_package_tree = Instant::now();
    let packages = package_tree::make(&filter, &project_root);
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

    let timing_source_files = Instant::now();
    print!(
        "{} {} Finding source files...",
        style("[2/7]").bold().dim(),
        LOOKING_GLASS
    );
    let _ = stdout().flush();
    let mut build_state = BuildState::new(project_root, root_config_name, packages);
    parse_packages(&mut build_state);
    logs::initialize(&build_state.project_root, &build_state.packages);
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
        "{} {} Cleaning up previous build...",
        style("[3/7]").bold().dim(),
        SWEEP
    );
    let timing_cleanup = Instant::now();
    let (diff_cleanup, total_cleanup, deleted_module_names) = clean::cleanup_previous_build(&mut build_state);
    let timing_cleanup_elapsed = timing_cleanup.elapsed();
    println!(
        "{}\r{} {}Cleaned {}/{} {:.2}s",
        LINE_CLEAR,
        style("[3/7]").bold().dim(),
        CHECKMARK,
        diff_cleanup,
        total_cleanup,
        default_timing.unwrap_or(timing_cleanup_elapsed).as_secs_f64()
    );

    let num_dirty_modules = build_state.modules.values().filter(|m| is_dirty(m)).count() as u64;

    let pb = ProgressBar::new(num_dirty_modules);
    pb.set_style(
        ProgressStyle::with_template(&format!(
            "{} {} Parsing... {{spinner}} {{pos}}/{{len}} {{msg}}",
            style("[4/7]").bold().dim(),
            CODE
        ))
        .unwrap(),
    );

    let timing_ast = Instant::now();
    let result_asts = generate_asts(&rescript_version, &mut build_state, &pb);
    let timing_ast_elapsed = timing_ast.elapsed();

    match result_asts {
        Ok(err) => {
            println!(
                "{}\r{} {}Parsed {} source files in {:.2}s",
                LINE_CLEAR,
                style("[4/7]").bold().dim(),
                CHECKMARK,
                num_dirty_modules,
                default_timing.unwrap_or(timing_ast_elapsed).as_secs_f64()
            );
            print!("{}", &err);
        }
        Err(err) => {
            logs::finalize(&build_state.project_root, &build_state.packages);
            println!(
                "{}\r{} {}Error parsing source files in {:.2}s",
                LINE_CLEAR,
                style("[4/7]").bold().dim(),
                CROSS,
                default_timing.unwrap_or(timing_ast_elapsed).as_secs_f64()
            );
            print!("{}", &err);
            clean::cleanup_after_build(&build_state);
            return Err(());
        }
    }

    let timing_deps = Instant::now();
    get_deps(&mut build_state, &deleted_module_names);
    let timing_deps_elapsed = timing_deps.elapsed();

    println!(
        "{}\r{} {}Collected deps in {:.2}s",
        LINE_CLEAR,
        style("[5/7]").bold().dim(),
        CHECKMARK,
        default_timing.unwrap_or(timing_deps_elapsed).as_secs_f64()
    );

    let start_compiling = Instant::now();

    let mut compiled_modules = AHashSet::<String>::new();
    let dirty_modules = build_state
        .modules
        .iter_mut()
        .filter_map(|(module_name, module)| {
            if module.compile_dirty {
                Some(module_name.to_owned())
            } else if !module.deps.is_disjoint(&deleted_module_names) {
                module.compile_dirty = true;
                Some(module_name.to_owned())
            } else {
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
    let mut num_compiled_modules = 0;
    let mut sorted_modules = build_state.module_names.iter().collect::<Vec<&String>>();
    sorted_modules.sort();

    // for module in dirty_modules.clone() {
    //     println!("dirty module: {}", module);
    // }

    // this is the whole "compile universe" all modules that might be dirty
    // we get this by traversing from the dirty modules to all the modules that
    // are dependent on them
    let mut compile_universe = dirty_modules.clone();

    let mut current_step_modules = dirty_modules.clone();
    loop {
        let mut reverse_deps: AHashSet<String> = AHashSet::new();
        for dirty_module in current_step_modules.iter() {
            reverse_deps.extend(build_state.get_module(dirty_module).unwrap().reverse_deps.clone());
        }
        current_step_modules = reverse_deps
            .difference(&compile_universe)
            .map(|s| s.to_string())
            .collect::<AHashSet<String>>();

        compile_universe.extend(current_step_modules.clone());
        if current_step_modules.is_empty() {
            break;
        }
    }
    let pb = ProgressBar::new(compile_universe.len().try_into().unwrap());
    pb.set_style(
        ProgressStyle::with_template(&format!(
            "{} {} Compiling... {{spinner}} {{pos}}/{{len}} {{msg}}",
            style("[6/7]").bold().dim(),
            SWORDS
        ))
        .unwrap(),
    );
    let compile_universe_count = compile_universe.len();

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

        in_progress_modules
            .clone()
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
                                &source_file.implementation.path,
                                &module.package_name,
                                &package.namespace,
                                &build_state.project_root,
                                "cmi",
                                package.is_root,
                            );

                            let cmi_digest = compute_file_hash(&cmi_path);

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
                                        &helpers::get_iast_path(
                                            &path,
                                            &package.name,
                                            &build_state.project_root,
                                            package.is_root,
                                        ),
                                        module,
                                        &build_state.project_root,
                                        &rescript_version,
                                        true,
                                    );
                                    Some(result)
                                }
                                _ => None,
                            };
                            let result = compile_file(
                                &package,
                                &root_package,
                                &helpers::get_ast_path(
                                    &source_file.implementation.path,
                                    &package.name,
                                    &build_state.project_root,
                                    package.is_root,
                                ),
                                module,
                                &build_state.project_root,
                                &rescript_version,
                                false,
                            );
                            // if let Err(error) = result.to_owned() {
                            //     println!("{}", error);
                            //     panic!("Implementation compilation error!");
                            // }
                            let cmi_digest_after = compute_file_hash(&cmi_path);

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
                        pb.inc(1);
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

                    let module_reverse_deps =
                        build_state.get_module(module_name).unwrap().reverse_deps.clone();

                    // if not clean -- compile modules that depend on this module
                    for dep in module_reverse_deps.iter() {
                        let dep_module = build_state.modules.get_mut(dep).unwrap();
                        //  mark the reverse dep as dirty when the source is not clean
                        if !*is_clean {
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
                                    logs::append(
                                        &build_state.project_root,
                                        package.is_root,
                                        &package.name,
                                        &err,
                                    );
                                    compile_warnings.push_str(&err);
                                }
                                Ok(None) => (),
                                Err(err) => {
                                    source_file.implementation.compile_state = CompileState::Error;
                                    logs::append(
                                        &build_state.project_root,
                                        package.is_root,
                                        &package.name,
                                        &err,
                                    );
                                    compile_errors.push_str(&err);
                                }
                            };
                            match interface_result {
                                Some(Ok(Some(err))) => {
                                    source_file.interface.as_mut().unwrap().compile_state =
                                        CompileState::Warning;
                                    logs::append(
                                        &build_state.project_root,
                                        package.is_root,
                                        &package.name,
                                        &err,
                                    );
                                    compile_warnings.push_str(&err);
                                }
                                Some(Ok(None)) => (),
                                Some(Err(err)) => {
                                    source_file.interface.as_mut().unwrap().compile_state =
                                        CompileState::Error;
                                    logs::append(
                                        &build_state.project_root,
                                        package.is_root,
                                        &package.name,
                                        &err,
                                    );
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
        if in_progress_modules.len() == 0 {
            // we probably want to find the cycle(s), and give a helpful error message here
            compile_errors.push_str("Can't continue... Dependency cycle\n")
        }
        if compile_errors.len() > 0 {
            break;
        };
    }
    let compile_duration = start_compiling.elapsed();

    logs::finalize(&build_state.project_root, &build_state.packages);
    pb.finish();
    clean::cleanup_after_build(&build_state);
    if compile_errors.len() > 0 {
        if helpers::contains_ascii_characters(&compile_warnings) {
            println!("{}", &compile_warnings);
        }
        println!(
            "{}\r{} {}Compiled {} modules in {:.2}s",
            LINE_CLEAR,
            style("[6/7]").bold().dim(),
            CROSS,
            num_compiled_modules,
            default_timing.unwrap_or(compile_duration).as_secs_f64()
        );
        print!("{}", &compile_errors);
        return Err(());
    } else {
        println!(
            "{}\r{} {}Compiled {} modules in {:.2}s",
            LINE_CLEAR,
            style("[6/7]").bold().dim(),
            CHECKMARK,
            num_compiled_modules,
            default_timing.unwrap_or(compile_duration).as_secs_f64()
        );
        if helpers::contains_ascii_characters(&compile_warnings) {
            print!("{}", &compile_warnings);
        }
    }

    let timing_total_elapsed = timing_total.elapsed();
    println!(
        "{}\r{} {}Finished Compilation in {:.2}s",
        LINE_CLEAR,
        style("[7/7]").bold().dim(),
        CHECKMARK,
        default_timing.unwrap_or(timing_total_elapsed).as_secs_f64()
    );

    Ok(build_state)
}
