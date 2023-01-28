pub mod bsconfig;
pub mod build;
pub mod structure_hashmap;
pub mod watcher;
use ahash::AHashMap;
use convert_case::{Case, Casing};
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

fn main() {
    let folder = "walnut_monorepo";
    // let build = create_build(folder);
    // println!("{:?}", build);
    //start_watcher(folder)
    let test_package = "walnut_monorepo/packages/stdlib";
    let build = create_build(&test_package);
    let root = &build[test_package];

    let file = "walnut_monorepo/packages/stdlib/src/Foo.res";
    let version_cmd = Command::new("walnut_monorepo/node_modules/rescript/rescript")
        .args(["-v"])
        .output()
        .expect("failed to find version");
    let version = std::str::from_utf8(&version_cmd.stdout)
        .expect("Could not read version from rescript")
        .replace("\n", "");
    let abs_node_modules_path = get_abs_path(&(folder.to_owned() + "/node_modules"));
    let pkg_path = get_abs_path(&(folder.to_owned() + "/packages/stdlib"));

    let build_path = get_abs_path(&(folder.to_owned() + "/packages/stdlib/_build"));
    let namespace = &root
        .bsconfig
        .name
        .to_owned()
        .replace("@", "")
        .replace("/", "_")
        .to_case(Case::Pascal);

    // we append the filename with the namespace with "-" -- this will not be used in the
    // generated js name (the AST file basename is informing the JS file name)!
    let ast_path = build_path.to_string()
        + "/"
        + &(get_basename(&file.to_string()).to_owned())
        + "-"
        + &namespace
        + ".ast";

    let ppx_flags = bsconfig::flatten_ppx_flags(&abs_node_modules_path, &root.bsconfig.ppx_flags);
    let bsc_flags = bsconfig::flatten_flags(&root.bsconfig.bsc_flags);
    let res_to_ast_args = vec![
        vec![
            "-bs-v".to_string(),
            format!("{}", version), // TODO - figure out what these string are. - Timestamps?
        ],
        ppx_flags,
        {
            root.bsconfig
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
    let res_to_ast = Command::new("walnut_monorepo/node_modules/rescript/darwinarm64/bsc.exe")
        .args(res_to_ast_args)
        .output()
        .expect("Error converting .res to .ast");
    println!("{}", std::str::from_utf8(&res_to_ast.stderr).expect(""));

    // /* Create .d */
    // let ast = file.replace(".res", ".ast");

    let ast_to_deps_args = vec![
        "-hash".to_string(),
        "e43be7fe8e2928155b6d87d24ae4006a".to_string(),
        "-bs-ns".to_string(),
        namespace.to_string(),
        ast_path.to_string(),
    ];

    // dbg!(&ast_to_deps_args);

    let ast_to_deps =
        Command::new("walnut_monorepo/node_modules/rescript/darwinarm64/bsb_helper.exe")
            .args(ast_to_deps_args)
            .output()
            .expect("err");

    println!("{}", std::str::from_utf8(&ast_to_deps.stderr).expect(""));

    // we skip this because we compile everything in a single dir
    // let deps = &root
    //     .bsconfig
    //     .bs_dependencies
    //     .as_ref()
    //     .unwrap_or(&vec![])
    //     .into_iter()
    //     .map(|x| {
    //         vec![
    //             "-I".to_string(),
    //             folder.to_string() + "/node_modules/" + x + "/lib/ocaml",
    //         ]
    //     })
    //     .collect::<Vec<Vec<String>>>();
    // dbg!(deps);

    // let sources = &root
    //     .source_files
    //     .as_ref()
    //     .map(|x| {
    //         x.keys()
    //             .into_iter()
    //             .map(|x| x.to_owned())
    //             .collect::<Vec<String>>()
    //     })
    //     .unwrap_or(vec![])
    //     .into_iter()
    //     .map(|x| vec!["-I".to_string(), x])
    //     .collect::<Vec<Vec<String>>>();

    let finger = &root
        .bsconfig
        .bs_dependencies
        .as_ref()
        .unwrap_or(&vec![])
        .into_iter()
        .map(|x| folder.to_owned() + "/node_modules/" + x + "/lib/ocaml/install.stamp")
        .collect::<Vec<String>>()
        .join(" ");

    let abs_file = get_abs_path(file);

    let to_mjs_args = vec![
        vec![
            // "-bs-ns".to_string(),
            // namespace.to_string(),
            "-I".to_string(),
            // PathBuf::from(&(folder.to_owned() + "walnut_monorepo/packages/stdlib")).to_string(),
            // "/Users/jfrolich/development/rewatch/walnut_monorepo/packages/stdlib".to_string(),
            build_path.to_string(),
        ],
        // sources.concat(),
        // deps.concat(),
        vec![
            "-bs-package-name".to_string(),
            root.bsconfig.name.to_owned(),
            "-bs-package-output".to_string(),
            format!("es6:{}:.mjs", "src"),
            "-bs-v".to_string(),
            finger.to_string(),
            ast_path.to_string(),
        ],
    ]
    .concat();

    dbg!(&to_mjs_args);

    let to_mjs = Command::new("../../node_modules/rescript/darwinarm64/bsc.exe")
        .current_dir(pkg_path.to_string())
        .args(to_mjs_args)
        .output()
        .expect("err");

    println!("STDOUT: {}", std::str::from_utf8(&to_mjs.stdout).expect(""));
    println!("STDERR: {}", std::str::from_utf8(&to_mjs.stderr).expect(""));
}
