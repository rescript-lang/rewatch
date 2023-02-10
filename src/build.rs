use crate::bsconfig;
use crate::helpers;
use crate::package_tree;
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

// Get the rescript version no. relative to project_root + `/node_modules/rescript/rescript`
pub fn get_version(project_root: &str) -> String {
    let version_cmd = Command::new(project_root.to_owned() + "/node_modules/rescript/rescript")
        .args(["-v"])
        .output()
        .expect("failed to find version");

    std::str::from_utf8(&version_cmd.stdout)
        .expect("Could not read version from rescript")
        .replace("\n", "")
}

// fn get_ast_path(file_path: &str, root_path: &str, package_name: &str) -> String {
//     return (get_basename(&file_path.to_string()).to_owned()) + ".ast";
// }

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
        &package.bsconfig.ppx_flags,
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
    let res_to_ast =
        Command::new(abs_node_modules_path.to_string() + "/rescript/darwinarm64/bsc.exe")
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
                .map(|path| helpers::file_path_to_module_name(&path, None))
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
                .map(|path| helpers::file_path_to_module_name(&path, package.namespace.to_owned()))
                .collect::<AHashSet<String>>();

            modules.insert(
                helpers::file_path_to_module_name(&mlmap.to_owned(), None),
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
                let module_name =
                    helpers::file_path_to_module_name(&file.to_owned(), namespace.to_owned());

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
    let abs_node_modules_path = helpers::get_node_modules_path(root_path);
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

    let _ = Command::new(
        abs_node_modules_path.to_string() + &"/rescript/darwinarm64/bsc.exe".to_string(),
    )
    .current_dir(build_path_abs.to_string())
    .args(args)
    .output()
    .expect("err");
}

pub fn compile_file(
    package_name: &str,
    ast_path: &str,
    source: &Module,
    root_path: &str,
    is_interface: bool,
) -> Option<String> {
    let build_path_abs = helpers::get_build_path(root_path, package_name);
    let pkg_path_abs = helpers::get_package_path(root_path, package_name);
    let abs_node_modules_path = helpers::get_node_modules_path(root_path);
    let bsc_flags = bsconfig::flatten_flags(&source.package.bsconfig.bsc_flags);

    let normal_deps = source
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

    let namespace_args = match source.namespace.to_owned() {
        Some(namespace) => vec!["-bs-ns".to_string(), namespace],
        None => vec![],
    };

    let read_cmi_args = match source.asti_path {
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
        debug!(
            "Compiling interface file: {}",
            &helpers::file_path_to_module_name(
                &source.interface_file_path.as_ref().unwrap(),
                source.namespace.to_owned()
            )
        );
        vec![]
    } else {
        debug!(
            "Compiling file: {}",
            &helpers::file_path_to_module_name(&source.file_path, source.namespace.to_owned())
        );
        vec![
            "-bs-package-name".to_string(),
            source.package.bsconfig.name.to_owned(),
            "-bs-package-output".to_string(),
            format!(
                "es6:{}:.mjs",
                Path::new(&source.file_path)
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

    let to_mjs = Command::new(
        abs_node_modules_path.to_string() + &"/rescript/darwinarm64/bsc.exe".to_string(),
    )
    .current_dir(build_path_abs.to_string())
    .args(to_mjs_args)
    .output();

    match to_mjs {
        Ok(x) if !x.status.success() => Some(std::str::from_utf8(&x.stderr).expect("").to_string()),
        Err(e) => Some(format!("ERROR, {}, {:?}", e, ast_path)),
        Ok(_) => None,
    }
}
