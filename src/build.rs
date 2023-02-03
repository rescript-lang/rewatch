use crate::bsconfig;
use crate::helpers::*;
use crate::package_tree;
use ahash::AHashMap;
use convert_case::{Case, Casing};
use rayon::prelude::*;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub dirty: bool,
    pub bsconfig: bsconfig::T,
}

pub fn get_version(project_root: &str) -> String {
    let version_cmd = Command::new(project_root.to_owned() + "/node_modules/rescript/rescript")
        .args(["-v"])
        .output()
        .expect("failed to find version");

    std::str::from_utf8(&version_cmd.stdout)
        .expect("Could not read version from rescript")
        .replace("\n", "")
}

pub fn create_ast(version: &str, project_root: &str, bsconfig: &bsconfig::T, file: &str) {
    // we append the filename with the namespace with "-" -- this will not be used in the
    // generated js name (the AST file basename is informing the JS file name)!
    let namespace = bsconfig
        .name
        .to_owned()
        .replace("@", "")
        .replace("/", "_")
        .to_case(Case::Pascal);

    let abs_node_modules_path = get_abs_path(&(project_root.to_owned() + "/node_modules"));
    let build_path = get_abs_path(&(project_root.to_owned() + "/packages/stdlib/_build"));
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
    Command::new("walnut_monorepo/node_modules/rescript/darwinarm64/bsc.exe")
        .args(args)
        .output()
        .expect("Error converting .res to .ast");
}

pub fn generate_asts(
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
                        bsconfig: package.bsconfig.to_owned(),
                    },
                );
            }),
        });

    files
        .par_iter()
        .for_each(|(file, metadata)| create_ast(&version, project_root, &metadata.bsconfig, file));

    files
}
