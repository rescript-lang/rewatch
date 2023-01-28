use crate::bsconfig;
use crate::helpers::*;
use crate::package_tree;
use ahash::AHashMap;
use convert_case::{Case, Casing};
use rayon::prelude::*;
use std::fs;
use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub dirty: bool,
    pub ast_path: Option<String>,
    pub ast_deps: Vec<String>,
    pub bsconfig: bsconfig::T,
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

// Create a single AST for a rescript version.
pub fn create_ast(version: &str, project_root: &str, bsconfig: &bsconfig::T, file: &str) -> String {
    // we append the filename with the namespace with "-" -- this will not be used in the
    // generated js name (the AST file basename is informing the JS file name)!
    let namespace = bsconfig
        .name
        .to_owned()
        .replace("@", "")
        .replace("/", "_")
        .to_case(Case::Pascal);

    let abs_node_modules_path = get_abs_path(&(project_root.to_owned() + "/node_modules"));

    let build_path_rel = &(project_root.to_owned() + "/node_modules/" + &bsconfig.name + "/_build");

    let _ = fs::create_dir(&build_path_rel);

    let build_path = get_abs_path(build_path_rel);
    let version_flags = vec![
        "-bs-v".to_string(),
        format!("{}", version), // TODO - figure out what these string are. - Timestamps?
    ];
    let ppx_flags = bsconfig::flatten_ppx_flags(&abs_node_modules_path, &bsconfig.ppx_flags);
    let bsc_flags = bsconfig::flatten_flags(&bsconfig.bsc_flags);
    let react_flags = bsconfig
        .reason
        .to_owned()
        .map(|x| vec!["-bs-jsx".to_string(), format!("{}", x.react_jsx)])
        .unwrap_or(vec![]);

    let ast_path = build_path.to_string()
        + "/"
        + &(get_basename(&file.to_string()).to_owned())
        + "-"
        + &namespace
        + ".ast";

    let file_args = vec![
        "-absname".to_string(),
        "-bs-ast".to_string(),
        "-o".to_string(),
        ast_path.to_string(),
        file.to_string(),
    ];

    let args = vec![version_flags, ppx_flags, react_flags, bsc_flags, file_args].concat();

    /* Create .ast */
    let ast = Command::new("walnut_monorepo/node_modules/rescript/darwinarm64/bsc.exe")
        .args(args)
        .output();

    //match ast {
    //Ok(x) => {
    //println!("STDOUT: {}", std::str::from_utf8(&x.stdout).expect(""));
    //println!("STDERR: {}", std::str::from_utf8(&x.stderr).expect(""));
    //}
    //Err(e) => {
    //println!("Could not compile: {:?}, ", e);
    //panic!("")
    //}
    //}

    ast_path
}

fn read_lines<P>(filename: P) -> io::Result<io::Lines<io::BufReader<File>>>
where
    P: AsRef<Path>,
{
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

fn get_dep_modules(ast_file: &str) -> Vec<String> {
    let mut deps = Vec::new();
    if let Ok(lines) = read_lines(ast_file) {
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
                Err(e) => println!("Error: {}", e),
            }
        }
    }
    return deps;
}

pub fn get_dependencies(
    version: String,
    project_root: &str,
    packages: AHashMap<String, package_tree::Package>,
) -> AHashMap<String, SourceFile> {
    let mut files: AHashMap<String, SourceFile> = AHashMap::new();

    packages
        .iter()
        .for_each(|(_package_name, package)| match &package.source_files {
            None => (),
            Some(source_files) => source_files.iter().for_each(|(file, _)| {
                files.insert(
                    file.to_owned(),
                    SourceFile {
                        dirty: true,
                        ast_path: None,
                        ast_deps: vec![],
                        bsconfig: package.bsconfig.to_owned(),
                    },
                );
            }),
        });

    files
        .par_iter()
        .map(|(file, metadata)| {
            let ast_path = create_ast(&version, project_root, &metadata.bsconfig, file);
            println!("{}", &ast_path);
            let ast_deps = get_dep_modules(&ast_path);

            (file.to_owned(), ast_path, ast_deps)
        })
        .collect::<Vec<(String, String, Vec<String>)>>()
        .into_iter()
        .for_each(|(file, ast_path, ast_deps)| {
            files.entry(file).and_modify(|file| {
                file.ast_path = Some(ast_path);
                file.ast_deps = ast_deps;
            });
        });

    files
}

pub fn compile_file(pkg_path_abs: &str, abs_node_modules_path: &str, source: &SourceFile) {
    let build_path_abs = &(pkg_path_abs.to_string() + &source.bsconfig.name + "/_build");
    let to_mjs_args = vec![
        vec!["-I".to_string(), ".".to_string()],
        vec![
            "-bs-package-name".to_string(),
            source.bsconfig.name.to_owned(),
            "-bs-package-output".to_string(),
            format!("es6:{}:.mjs", "src"),
            source.ast_path.to_owned().expect("No path found"),
        ],
    ]
    .concat();

    let to_mjs = Command::new(
        abs_node_modules_path.to_string() + &"/rescript/darwinarm64/bsc.exe".to_string(),
    )
    .current_dir(build_path_abs.to_string())
    .args(to_mjs_args)
    .output();

    match to_mjs {
        Ok(x) => {
            //println!("STDOUT: {}", std::str::from_utf8(&x.stdout).expect(""));
            println!("STDERR: {}", std::str::from_utf8(&x.stderr).expect(""));
        }
        Err(e) => println!("ERROR, {}, {:?}", e, source.ast_path),
    }
}
