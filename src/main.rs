pub mod bsconfig;
pub mod build;
pub mod helpers;
pub mod package_tree;
pub mod structure_hashmap;
pub mod watcher;
use ahash::AHashMap;
use convert_case::{Case, Casing};
use std::fs;
use std::fs::File;
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::process::Command;

fn get_abs_path(path: &str) -> String {
    let abs_path_buf = PathBuf::from(path);
    return fs::canonicalize(abs_path_buf)
        .expect("Could not canonicalize")
        .to_str()
        .expect("Could not canonicalize")
        .to_string();
}

fn get_basename(path: &str) -> String {
    let path_buf = PathBuf::from(path);
    return path_buf
        .file_stem()
        .expect("Could not get basename")
        .to_str()
        .expect("Could not get basename")
        .to_string();
}

fn get_namespace(package: &package_tree::Package) -> Option<String> {
    if package.namespace {
        return Some(
            package
                .bsconfig
                .name
                .to_owned()
                .replace("@", "")
                .replace("/", "_")
                .to_case(Case::Pascal),
        );
    }
    return None;
}

fn get_package_path(root: &str, package_name: &str) -> String {
    return format!("{}/node_modules/{}", root, package_name);
}

fn get_build_path(root: &str, package_name: &str) -> String {
    return format!("{}/node_modules/{}/_build", root, package_name);
}

fn get_path(root: &str, package_name: &str, file: &str) -> String {
    return format!("{}/{}/{}", root, package_name, file);
}

fn get_node_modules_path(root: &str) -> String {
    return format!("{}/node_modules", root);
}

fn generate_ast(
    package: &package_tree::Package,
    filename: &str,
    root_path: &str,
    version: &str,
) -> String {
    let file = &("..".to_string() + &filename.to_string());
    let build_path_abs = get_build_path(root_path, &package.name);
    let ast_path = (get_basename(&file.to_string()).to_owned()) + ".ast";
    let abs_node_modules_path = get_node_modules_path(root_path);

    let ppx_flags =
        bsconfig::flatten_ppx_flags(&abs_node_modules_path, &package.bsconfig.ppx_flags);
    let bsc_flags = bsconfig::flatten_flags(&package.bsconfig.bsc_flags);
    let res_to_ast_args = vec![
        vec![
            "-bs-v".to_string(),
            format!("{}", version), // TODO - figure out what these string are. - Timestamps?
        ],
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

    dbg!(build_path_abs.to_string());
    /* Create .ast */
    dbg!(&res_to_ast_args);
    let res_to_ast = Command::new(
        abs_node_modules_path.to_string() + &"/rescript/darwinarm64/bsc.exe".to_string(),
    )
    .current_dir(build_path_abs.to_string())
    .args(res_to_ast_args)
    .output()
    .expect("Error converting .res to .ast");
    println!(
        "{}",
        std::str::from_utf8(&res_to_ast.stderr).expect("Failure")
    );
    return ast_path;
}

fn compile(package: &package_tree::Package, ast_path: &str, root_path: &str) {
    let abs_node_modules_path = get_node_modules_path(root_path);
    let namespace = get_namespace(package);
    let to_mjs_args = vec![
        match namespace {
            Some(namespace) => vec!["-bs-ns".to_string(), namespace.to_string()],
            None => vec![],
        },
        vec!["-I".to_string(), ".".to_string()],
        vec![
            "-bs-package-name".to_string(),
            package.bsconfig.name.to_owned(),
            "-bs-package-output".to_string(),
            // "src" here needs to be the relative folder name of the mjs file
            format!("es6:{}:.mjs", "src"),
            ast_path.to_string(),
        ],
    ]
    .concat();

    dbg!(&to_mjs_args);
    let build_path_abs = get_build_path(root_path, &package.name);

    let to_mjs = Command::new(
        abs_node_modules_path.to_string() + &"/rescript/darwinarm64/bsc.exe".to_string(),
    )
    .current_dir(build_path_abs.to_string())
    .args(to_mjs_args)
    .output()
    .expect("err");

    println!("STDOUT: {}", std::str::from_utf8(&to_mjs.stdout).expect(""));
    println!("STDERR: {}", std::str::from_utf8(&to_mjs.stderr).expect(""));
}

fn main() {
    let folder = "walnut_monorepo";

    let packages = package_tree::make(&folder);
    let rescript_version = build::get_version(&folder);
    let source_files = build::get_dependencies(rescript_version, &folder, packages.to_owned());
    let project_root = get_abs_path("walnut_monorepo");

    let root = &packages["node_modules/@teamwalnut/stdlib"];

    let version_cmd = Command::new("node_modules/rescript/rescript")
        .current_dir(project_root.to_string())
        .args(["-v"])
        .output()
        .expect("failed to find version");

    let version = std::str::from_utf8(&version_cmd.stdout)
        .expect("Could not read version from rescript")
        .replace("\n", "");

    let ast_file = generate_ast(root, "/src/Bar.res", &project_root, &version);
    compile(root, &ast_file, &project_root);

    let ast_file = generate_ast(root, "/src/Foo.res", &project_root, &version);
    compile(root, &ast_file, &project_root);
}
