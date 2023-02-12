use crate::bsconfig;
use crate::bsconfig::OneOrMore;
use crate::helpers;
use crate::helpers::get_bs_build_path;
use crate::helpers::get_build_path;
use crate::helpers::get_package_path;
use crate::package_tree;
use crate::package_tree::Package;
use ahash::{AHashMap, AHashSet};
use log::{debug, error};
use rayon::prelude::*;
use std::fs;
use std::fs::File;
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub enum SourceType {
    SourceFile,
    MlMap,
}

#[derive(Debug, Clone)]
pub struct Module {
    pub dirty: bool,
    pub source_type: SourceType,
    pub namespace: Option<String>,
    pub file_path: String,
    pub interface_file_path: Option<String>,
    pub ast_path: Option<String>,
    pub asti_path: Option<String>,
    pub deps: AHashSet<String>,
    pub package: package_tree::Package,
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

fn get_compiler_asset(
    source_file: &str,
    package_name: &str,
    namespace: &Option<String>,
    root_path: &str,
    extension: &str,
) -> String {
    let namespace = match extension {
        "ast" | "asti" => &None,
        _ => namespace,
    };

    get_build_path(root_path, package_name)
        + "/"
        + &helpers::file_path_to_compiler_asset_basename(source_file, namespace)
        + "."
        + extension
}

fn get_bs_compiler_asset(
    source_file: &str,
    package_name: &str,
    namespace: &Option<String>,
    root_path: &str,
    extension: &str,
) -> String {
    let namespace = match extension {
        "ast" | "iast" => &None,
        _ => namespace,
    };
    let dir = std::path::Path::new(source_file)
        .strip_prefix(get_package_path(root_path, &package_name))
        .unwrap()
        .parent()
        .unwrap();

    std::path::Path::new(&get_bs_build_path(root_path, &package_name))
        .join(dir)
        .join(helpers::file_path_to_compiler_asset_basename(source_file, namespace) + extension)
        .to_str()
        .unwrap()
        .to_owned()
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
    for extension in &["cmj", "cmi", "cmt", "cmti", "ast", "iast"] {
        let _ = std::fs::remove_file(get_compiler_asset(
            source_file,
            package_name,
            namespace,
            root_path,
            extension,
        ));
        if ["cmj", "cmi", "cmt", "cmti"].contains(&extension) {
            let _ = std::fs::remove_file(get_bs_compiler_asset(
                source_file,
                package_name,
                namespace,
                root_path,
                extension,
            ));
        }
    }
}

pub fn cleanup_previous_build(
    packages: &AHashMap<String, Package>,
    all_modules: &AHashMap<String, Module>,
    root_path: &str,
) -> (usize, usize) {
    let mut ast_modules: AHashMap<String, (String, String, Option<String>)> = AHashMap::new();
    let mut ast_rescript_file_locations = AHashSet::new();

    let mut rescript_file_locations = all_modules
        .values()
        .filter(|module| module.source_type == SourceType::SourceFile)
        .map(|module| module.file_path.to_owned())
        .collect::<AHashSet<String>>();

    rescript_file_locations.extend(
        all_modules
            .values()
            .filter(|module| module.source_type == SourceType::SourceFile)
            .filter_map(|module| module.interface_file_path.to_owned())
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
        let (_module_name, package_name, package_namespace) = ast_modules
            .get(&res_file_location.to_string())
            .expect("Could not find module name for ast file");
        remove_compile_assets(
            res_file_location,
            package_name,
            package_namespace,
            root_path,
        );
    });

    (diff_len, ast_rescript_file_locations.len())
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
) -> String {
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

    let stderr = std::str::from_utf8(&res_to_ast.stderr).expect("");
    if helpers::contains_ascii_characters(stderr) {
        debug!("Output contained ASCII Characters: {:?}", stderr)
    }

    ast_path
}

fn read_lines(filename: String) -> io::Result<io::Lines<io::BufReader<File>>> {
    let file = File::open(filename)?;
    Ok(io::BufReader::new(file).lines())
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
    let digest = "randjbuildsystem".to_owned() + "\n" + &modules.join("\n");
    let file = build_path_abs.to_string() + "/" + namespace + ".mlmap";
    fs::write(&file, digest).expect("Unable to write mlmap");
    file.to_string()
}

pub fn generate_asts(
    version: String,
    project_root: &str,
    mut modules: AHashMap<String, Module>,
    all_modules: AHashSet<String>,
) -> AHashMap<String, Module> {
    modules
        .par_iter()
        .map(|(module_name, metadata)| {
            debug!("Generating AST for module: {}", module_name);
            match metadata.source_type {
                SourceType::MlMap => (
                    module_name.to_owned(),
                    metadata.ast_path.to_owned().unwrap(),
                    None,
                    metadata.deps.to_owned(),
                ),

                SourceType::SourceFile => {
                    let ast_path = generate_ast(
                        metadata.package.to_owned(),
                        &metadata.file_path.to_owned(),
                        &helpers::get_abs_path(project_root),
                        &version,
                    );

                    let asti_path = match metadata.interface_file_path.to_owned() {
                        Some(interface_file_path) => Some(generate_ast(
                            metadata.package.to_owned(),
                            &interface_file_path.to_owned(),
                            &helpers::get_abs_path(project_root),
                            &version,
                        )),
                        _ => None,
                    };

                    let build_path =
                        helpers::get_build_path(project_root, &metadata.package.bsconfig.name);

                    // choose the namespaced dep if that module appears in the package, otherwise global dep
                    let mut deps = get_dep_modules(
                        &(build_path.to_string() + "/" + &ast_path),
                        metadata.namespace.to_owned(),
                        &metadata.package.modules.as_ref().unwrap(),
                        &all_modules,
                    );
                    match asti_path.to_owned() {
                        Some(asti_path) => deps.extend(get_dep_modules(
                            &(build_path.to_owned() + "/" + &asti_path),
                            metadata.namespace.to_owned(),
                            &metadata.package.modules.as_ref().unwrap(),
                            &all_modules,
                        )),
                        None => (),
                    }
                    deps.remove(module_name);

                    (module_name.to_owned(), ast_path, asti_path, deps)
                }
            }
        })
        .collect::<Vec<(String, String, Option<String>, AHashSet<String>)>>()
        .into_iter()
        .for_each(|(module_name, ast_path, asti_path, deps)| {
            modules.entry(module_name).and_modify(|module| {
                module.ast_path = Some(ast_path);
                module.asti_path = asti_path;
                module.deps = deps;
            });
        });

    modules
}

pub fn parse(
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

            compile_mlmap(&package, namespace, &project_root);

            let deps = source_files
                .iter()
                .map(|path| helpers::file_path_to_module_name(&path, &package.namespace))
                .collect::<AHashSet<String>>();

            modules.insert(
                helpers::file_path_to_module_name(&mlmap.to_owned(), &None),
                Module {
                    file_path: mlmap.to_owned(),
                    interface_file_path: None,
                    dirty: true,
                    source_type: SourceType::MlMap,
                    namespace: None,
                    ast_path: Some(mlmap.to_owned()),
                    asti_path: None,
                    deps,
                    package: package.to_owned(),
                },
            );
        });

        debug!("Building source file-tree for package: {}", package.name);
        match &package.source_files {
            None => (),
            Some(source_files) => source_files.iter().for_each(|(file, _)| {
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
                        .and_modify(|module| {
                            if module.file_path.len() > 0 {
                                error!("Duplicate files found for module: {}", &module_name);

                                panic!("Unable to continue... See log output above...");
                            }
                            module.file_path = file.to_owned();
                        })
                        .or_insert(Module {
                            file_path: file.to_owned(),
                            interface_file_path: None,
                            dirty: true,
                            source_type: SourceType::SourceFile,
                            namespace,
                            ast_path: None,
                            asti_path: None,
                            deps: AHashSet::new(),
                            package: package.to_owned(),
                        });
                } else {
                    modules
                        .entry(module_name.to_string())
                        .and_modify(|module| module.interface_file_path = Some(file.to_owned()))
                        .or_insert(Module {
                            file_path: "".to_string(),
                            interface_file_path: Some(file.to_owned()),
                            dirty: true,
                            source_type: SourceType::SourceFile,
                            namespace,
                            ast_path: None,
                            asti_path: None,
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
) -> Result<(), String> {
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

    let namespace_args = match module.namespace.to_owned() {
        Some(namespace) => vec!["-bs-ns".to_string(), namespace],
        None => vec![],
    };

    let read_cmi_args = match module.asti_path {
        Some(_) => {
            if is_interface {
                vec![]
            } else {
                vec!["-bs-read-cmi".to_string()]
            }
        }
        _ => vec![],
    };

    let module_name = helpers::file_path_to_module_name(&module.file_path, &module.namespace);

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
                Path::new(&module.file_path)
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
        Ok(x) if !x.status.success() => Err(std::str::from_utf8(&x.stderr).expect("").to_string()),
        Err(e) => Err(format!("ERROR, {}, {:?}", e, ast_path)),
        Ok(_) => {
            let dir = std::path::Path::new(&module.file_path)
                .strip_prefix(get_package_path(root_path, &module.package.name))
                .unwrap()
                .parent()
                .unwrap();
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
            Ok(())
        }
    }
}
