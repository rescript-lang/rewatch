use crate::bsconfig;
use crate::bsconfig::OneOrMore;
use crate::helpers;
use crate::helpers::get_bs_build_path;
use crate::helpers::get_package_path;
use crate::package_tree;
use crate::package_tree::Package;
use ahash::{AHashMap, AHashSet};
use console::{style, Emoji};
use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use log::Level::Info;
use log::{debug, error};
use log::{info, log_enabled};
use rayon::prelude::*;
use std::fs;
use std::io::stdout;
use std::io::Write;
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use std::time::SystemTime;

static TREE: Emoji<'_, '_> = Emoji("🌴 ", "");
static SWEEP: Emoji<'_, '_> = Emoji("🧹 ", "");
static LOOKING_GLASS: Emoji<'_, '_> = Emoji("🔍 ", "");
static CODE: Emoji<'_, '_> = Emoji("🟰  ", "");
static SWORDS: Emoji<'_, '_> = Emoji("⚔️  ", "");
static CHECKMARK: Emoji<'_, '_> = Emoji("️✅  ", "");
static CROSS: Emoji<'_, '_> = Emoji("️🛑  ", "");
static LINE_CLEAR: &str = "\x1b[2K";

#[derive(Debug, Clone, PartialEq)]
pub enum ParseState {
    Pending,
    ParseError,
    Warning,
    Success,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompileState {
    Pending,
    Error,
    Warning,
    Success,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Interface {
    path: String,
    parse_state: ParseState,
    compile_state: CompileState,
    last_modified: SystemTime,
    dirty: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Implementation {
    path: String,
    parse_state: ParseState,
    compile_state: CompileState,
    last_modified: SystemTime,
    dirty: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceFile {
    implementation: Implementation,
    interface: Option<Interface>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MlMap {
    dirty: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SourceType {
    SourceFile(SourceFile),
    MlMap(MlMap),
}

#[derive(Debug, Clone)]
pub struct Module {
    pub source_type: SourceType,
    pub deps: AHashSet<String>,
    pub package: package_tree::Package,
}

fn read_lines(filename: String) -> io::Result<io::Lines<io::BufReader<fs::File>>> {
    let file = fs::File::open(filename)?;
    Ok(io::BufReader::new(file).lines())
}

fn get_res_path_from_ast(ast_file: &str) -> Option<String> {
    if let Ok(lines) = read_lines(ast_file.to_string()) {
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

fn remove_asts(source_file: &str, package_name: &str, namespace: &Option<String>, root_path: &str) {
    let _ = std::fs::remove_file(helpers::get_compiler_asset(
        source_file,
        package_name,
        namespace,
        root_path,
        "ast",
    ));
    let _ = std::fs::remove_file(helpers::get_compiler_asset(
        source_file,
        package_name,
        namespace,
        root_path,
        "iast",
    ));
}

fn get_interface<'a>(module: &'a Module) -> &'a Option<Interface> {
    match &module.source_type {
        SourceType::SourceFile(source_file) => &source_file.interface,
        _ => &None,
    }
}

fn remove_compile_assets(
    source_file: &str,
    package_name: &str,
    namespace: &Option<String>,
    root_path: &str,
) {
    let _ = std::fs::remove_file(helpers::change_extension(source_file, "mjs"));
    // optimization
    // only issue cmti if htere is an interfacce file
    for extension in &["cmj", "cmi", "cmt", "cmti"] {
        let _ = std::fs::remove_file(helpers::get_compiler_asset(
            source_file,
            package_name,
            namespace,
            root_path,
            extension,
        ));
        let _ = std::fs::remove_file(helpers::get_bs_compiler_asset(
            source_file,
            package_name,
            namespace,
            root_path,
            extension,
        ));
    }
}

pub fn cleanup_previous_build(
    packages: &AHashMap<String, Package>,
    all_modules: &mut AHashMap<String, Module>,
    root_path: &str,
) -> (usize, usize, AHashSet<String>) {
    let mut ast_modules: AHashMap<String, (String, String, Option<String>, SystemTime, String)> =
        AHashMap::new();
    let mut ast_rescript_file_locations = AHashSet::new();

    let mut rescript_file_locations = all_modules
        .values()
        .filter_map(|module| match &module.source_type {
            SourceType::SourceFile(source_file) => {
                Some(source_file.implementation.path.to_string())
            }
            _ => None,
        })
        .collect::<AHashSet<String>>();

    rescript_file_locations.extend(
        all_modules
            .values()
            .filter_map(|module| {
                get_interface(module)
                    .as_ref()
                    .map(|interface| interface.path.to_string())
            })
            .collect::<AHashSet<String>>(),
    );

    // scan all ast files in all packages
    for package in packages.values() {
        let read_dir = fs::read_dir(std::path::Path::new(&helpers::get_build_path(
            root_path,
            &package.name,
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
                                            ),
                                        );
                                        let _ = ast_rescript_file_locations.insert(res_file_path);
                                    }
                                    None => (),
                                }
                            }
                            _ => (),
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

    diff.par_iter().for_each(|res_file_location| {
        let _ = std::fs::remove_file(helpers::change_extension(res_file_location, "mjs"));
        let (_module_name, package_name, package_namespace, _last_modified, _ast_file_path) =
            ast_modules
                .get(&res_file_location.to_string())
                .expect("Could not find module name for ast file");
        remove_asts(
            res_file_location,
            package_name,
            package_namespace,
            root_path,
        );
        remove_compile_assets(
            res_file_location,
            package_name,
            package_namespace,
            root_path,
        );
    });

    ast_rescript_file_locations
        .intersection(&rescript_file_locations)
        .into_iter()
        .for_each(|res_file_location| {
            let (module_name, _package_name, _package_namespace, ast_last_modified, ast_file_path) =
                ast_modules
                    .get(res_file_location)
                    .expect("Could not find module name for ast file");
            let module = all_modules
                .get_mut(module_name)
                .expect("Could not find module for ast file");

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
                        if ast_last_modified > &source_last_modified {
                            implementation.dirty = false;
                        }
                    }
                }
            }
        });

    let ast_module_names = ast_modules
        .values()
        .map(|(module_name, _, _, _, _)| module_name)
        .collect::<AHashSet<&String>>();

    let all_module_names = all_modules
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
    if let Ok(lines) = read_lines(ast_file.to_string()) {
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

    modules
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
                        false,
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
                                helpers::get_ast_path(
                                    &source_file.implementation.path,
                                    &module.package.name,
                                    &project_root,
                                ),
                                None,
                            )),
                            Ok(source_file.interface.as_ref().map(|i| {
                                (
                                    helpers::get_iast_path(
                                        &i.path,
                                        &module.package.name,
                                        &project_root,
                                    ),
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

                    let has_dirty_namespace = match module.package.namespace.to_owned() {
                        Some(namespace) => deleted_modules.contains(&namespace),
                        None => false,
                    };

                    let has_dirty_deps = !deps.is_disjoint(deleted_modules) || has_dirty_namespace;

                    (
                        module_name.to_owned(),
                        ast_path,
                        asti_path,
                        deps,
                        has_dirty_deps,
                    )
                }
            }
        })
        .collect::<Vec<(
            String,
            Result<(String, Option<String>), String>,
            Result<Option<(String, Option<String>)>, String>,
            AHashSet<String>,
            bool,
        )>>()
        .into_iter()
        .for_each(|(module_name, ast_path, iast_path, deps, has_dirty_deps)| {
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

                if has_dirty_deps {
                    match module.source_type {
                        SourceType::SourceFile(ref mut source_file) => {
                            source_file.implementation.dirty = true;
                            source_file.interface.as_mut().map(|interface| {
                                interface.dirty = true;
                            });
                        }
                        SourceType::MlMap(ref mut mlmap) => {
                            mlmap.dirty = true;
                        }
                    }
                }
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
                    source_type: SourceType::MlMap(MlMap { dirty: true }),
                    deps: deps,
                    package: package.to_owned(),
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
        vec!["-w".to_string(), "a".to_string()],
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
            let dir = std::path::Path::new(implementation_file_path)
                .strip_prefix(get_package_path(root_path, &module.package.name))
                .unwrap()
                .parent()
                .unwrap();

            // perhaps we can do this copying somewhere else
            if !is_interface {
                let _ = std::fs::copy(
                    build_path_abs.to_string() + "/" + &module_name + ".cmi",
                    std::path::Path::new(&get_bs_build_path(root_path, &module.package.name))
                        .join(dir)
                        .join(module_name.to_owned() + ".cmi"),
                );
                let _ = std::fs::copy(
                    build_path_abs.to_string() + "/" + &module_name + ".cmj",
                    std::path::Path::new(&get_bs_build_path(root_path, &module.package.name))
                        .join(dir)
                        .join(module_name.to_owned() + ".cmj"),
                );
                let _ = std::fs::copy(
                    build_path_abs.to_string() + "/" + &module_name + ".cmt",
                    std::path::Path::new(&get_bs_build_path(root_path, &module.package.name))
                        .join(dir)
                        .join(module_name.to_owned() + ".cmt"),
                );
            } else {
                let _ = std::fs::copy(
                    build_path_abs.to_string() + "/" + &module_name + ".cmti",
                    std::path::Path::new(&get_bs_build_path(root_path, &module.package.name))
                        .join(dir)
                        .join(module_name.to_owned() + ".cmti"),
                );
            }

            let stderr = std::str::from_utf8(&x.stderr).expect("stderr should be non-null");
            if helpers::contains_ascii_characters(stderr) {
                Ok(Some(stderr.to_string()))
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

fn cleanup_after_build(
    modules: &AHashMap<String, Module>,
    compiled_modules: &AHashSet<String>,
    all_modules: &AHashSet<String>,
    project_root: &str,
) {
    let failed_modules = all_modules
        .difference(&compiled_modules)
        .collect::<AHashSet<&String>>();

    let cleanup = modules
        .iter()
        .filter(|(module_name, module)| {
            failed_to_compile(module) || failed_modules.contains(module_name)
        })
        .map(|_| ())
        .collect::<Vec<()>>()
        .len();
    dbg!(cleanup);

    modules.par_iter().for_each(|(module_name, module)| {
        if failed_to_compile(module) || failed_modules.contains(module_name) {
            // only retain ast file if it compiled successfully, that's the only thing we check
            // if we see a AST file, we assume it compiled successfully, so we also need to clean
            // up the AST file if compile is not successful
            match &module.source_type {
                SourceType::SourceFile(source_file) => {
                    remove_asts(
                        &source_file.implementation.path,
                        &module.package.name,
                        &module.package.namespace,
                        &project_root,
                    );
                    remove_compile_assets(
                        &source_file.implementation.path,
                        &module.package.name,
                        &module.package.namespace,
                        &project_root,
                    );
                }
                _ => (),
            }
        }
    });
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
        cleanup_previous_build(&packages, &mut modules, &project_root);
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

    dbg!(modules
        .iter()
        .filter(|(_, m)| match m.source_type {
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
        })
        .map(|_| ())
        .collect::<Vec<()>>()
        .len());

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
            cleanup_after_build(&modules, &AHashSet::new(), &all_modules, &project_root);
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

    // let mut compiled_modules = AHashSet::<String>::new();
    let mut compiled_modules = modules
        .iter()
        .filter_map(|(module_name, module)| {
            if is_dirty(module) {
                None
            } else {
                Some(module_name.to_owned())
            }
        })
        .collect::<AHashSet<String>>();

    let mut loop_count = 0;
    let mut files_total_count = compiled_modules.len();
    dbg!(&files_total_count);
    let mut files_current_loop_count;
    let mut compile_errors = "".to_string();
    let mut compile_warnings = "".to_string();
    let total_modules = modules.len();

    loop {
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
                if module.deps.is_subset(&compiled_modules)
                    && !compiled_modules.contains(module_name)
                {
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
                            ))
                        }
                        SourceType::SourceFile(source_file) => {
                            // compile interface first
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

                            Some((module_name.to_owned(), result, interface_result))
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
                )>,
            >>()
            .iter()
            .for_each(|result| match result {
                Some((module_name, result, interface_result)) => {
                    if !(log_enabled!(Info)) {
                        pb.inc(1);
                    }
                    files_current_loop_count += 1;
                    compiled_modules.insert(module_name.to_string());

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
    cleanup_after_build(&modules, &compiled_modules, &all_modules, &project_root);
    if helpers::contains_ascii_characters(&compile_warnings) {
        println!("{}", &compile_warnings);
    }
    if compile_errors.len() > 0 {
        println!(
            "{}\r{} {}Compiled in {:.2}s",
            LINE_CLEAR,
            style("[5/5]").bold().dim(),
            CROSS,
            compile_duration.as_secs_f64()
        );
        println!("{}", &compile_errors);
        return Err(());
    } else {
        println!(
            "{}\r{} {}Compiled in {:.2}s",
            LINE_CLEAR,
            style("[5/5]").bold().dim(),
            CHECKMARK,
            compile_duration.as_secs_f64()
        );
    }

    let timing_total_elapsed = timing_total.elapsed();
    println!("Done in {:.2}s", timing_total_elapsed.as_secs_f64());

    Ok(modules)
}
