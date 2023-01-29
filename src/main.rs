pub mod bsconfig;
pub mod build;
pub mod structure_hashmap;
pub mod watcher;
use ahash::AHashMap;
use convert_case::{Case, Casing};
use linked_hash_set::LinkedHashSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn create_build(folder: &str) -> AHashMap<String, build::Package> {
    /* By Extending, we should eventually be able to parallalize */
    let mut map: AHashMap<String, build::Package> = AHashMap::new();
    map.extend(build::make(folder, None));

    for (_key, value) in map.iter_mut() {
        /* We may want to directly build a reverse-lookup from filename -> package while we do this */
        let mut map: AHashMap<String, fs::Metadata> = AHashMap::new();
        value.source_folders.iter().for_each(|(dir, source)| {
            map.extend(build::read_files(dir, source));
        });

        value.source_files = Some(map);
    }

    map
}

fn start_watcher(folder: &str) {
    futures::executor::block_on(async {
        if let Err(e) = watcher::async_watch(folder).await {
            println!("error: {:?}", e)
        }
    });
}

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

fn create_order(build: &AHashMap<String, build::Package>) -> Vec<LinkedHashSet<build::Package>> {
    let mut build_order = vec![LinkedHashSet::new()];

    build
        .iter()
        .filter(|(_name, package)| match &package.bsconfig.bs_dependencies {
            None => true,
            Some(xs) if xs.len() == 0 => true,
            _ => false,
        })
        .for_each(|(_name, package)| {
            build_order[0].insert(package.to_owned());
        });

    let mut level = 0;
    while build_order[level].len() != 0 {
        let mut parents = LinkedHashSet::new();

        build_order[level]
            .to_owned()
            .into_iter()
            .for_each(|package| {
                package
                    .parent
                    .as_ref()
                    .and_then(|parent| build.get(parent))
                    .into_iter()
                    .for_each(|parent| {
                        parents.insert(parent.clone());
                    })
            });

        build_order.push(parents);
        level += 1
    }

    build_order
}

fn main() {
    let folder = "walnut_monorepo";
    let build = create_build(&folder);

    let order = create_order(&build);

    order.into_iter().enumerate().for_each(|(i, package)| {
        println!("Compiling Level: {i}");
        package.into_iter().for_each(|package| {
            println!("Compiling Package: {}", package.name);
            package
                .source_files
                .as_ref()
                .expect("No source files found")
                .iter()
                .for_each(|(file, _metadata)| {
                    let version_cmd =
                        Command::new("walnut_monorepo/node_modules/rescript/rescript")
                            .args(["-v"])
                            .output()
                            .expect("failed to find version");
                    let version = std::str::from_utf8(&version_cmd.stdout)
                        .expect("Could not read version from rescript")
                        .replace("\n", "");
                    let abs_node_modules_path =
                        get_abs_path(&(folder.to_owned() + "/node_modules"));
                    let pkg_path =
                        get_abs_path(&(folder.to_owned() + "/node_modules/" + &package.name));

                    let _ = fs::create_dir(pkg_path.to_string() + "/_build");
                    let build_path = get_abs_path(&(pkg_path.to_owned() + "/_build"));
                    let namespace = &package
                        .bsconfig
                        .name
                        .to_owned()
                        .replace("@", "")
                        .replace("/", "_")
                        .to_case(Case::Pascal);

                    let ast_path = build_path.to_string()
                        + "/"
                        + &(get_basename(&file.to_string()).to_owned())
                        + "-"
                        + &namespace
                        + ".ast";

                    let ppx_flags = bsconfig::flatten_ppx_flags(
                        &abs_node_modules_path,
                        &package.bsconfig.ppx_flags,
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
                        Command::new("walnut_monorepo/node_modules/rescript/darwinarm64/bsc.exe")
                            .args(res_to_ast_args)
                            .output()
                            .expect("Error converting .res to .ast");
                    //println!("{}", std::str::from_utf8(&res_to_ast.stderr).expect(""));

                    let ast_to_deps_args = vec![
                        "-hash".to_string(),
                        "e43be7fe8e2928155b6d87d24ae4006a".to_string(),
                        "-bs-ns".to_string(),
                        namespace.to_string(),
                        ast_path.to_string(),
                    ];

                    let ast_to_deps = Command::new(
                        "walnut_monorepo/node_modules/rescript/darwinarm64/bsb_helper.exe",
                    )
                    .args(ast_to_deps_args)
                    .output()
                    .expect("err");

                    let finger = &package
                        .bsconfig
                        .bs_dependencies
                        .as_ref()
                        .unwrap_or(&vec![])
                        .into_iter()
                        .map(|x| {
                            folder.to_owned() + "/node_modules/" + x + "/lib/ocaml/install.stamp"
                        })
                        .collect::<Vec<String>>()
                        .join(" ");

                    let to_mjs_args = vec![
                        vec!["-I".to_string(), build_path.to_string()],
                        vec![
                            "-bs-package-name".to_string(),
                            package.bsconfig.name.to_owned(),
                            "-bs-package-output".to_string(),
                            format!("es6:{}:.mjs", "src"),
                            "-bs-v".to_string(),
                            finger.to_string(),
                            ast_path.to_string(),
                        ],
                    ]
                    .concat();

                    println!("{:?}", to_mjs_args);
                    println!("{}", pkg_path);
                    let to_mjs = Command::new("../../node_modules/rescript/darwinarm64/bsc.exe")
                        .current_dir(pkg_path.to_string())
                        .args(to_mjs_args)
                        .output()
                        .expect("err");

                    //std::str::from_utf8(&to_mjs.stdout)
                    //.iter()
                    //.for_each(|ln| println!("{}", ln));
                    //std::str::from_utf8(&to_mjs.stderr)
                    //.iter()
                    //.for_each(|ln| println!("{}", ln));
                })
        });
    });
    // Start with the leaf elements (without children), then recurse upwards
}
