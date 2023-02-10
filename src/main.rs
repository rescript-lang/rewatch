pub mod bsconfig;
pub mod build;
pub mod helpers;
pub mod package_tree;
pub mod structure_hashmap;
pub mod watcher;
use ahash::AHashSet;
use rayon::prelude::*;

fn clean() {}

fn main() {
    let project_root = helpers::get_abs_path("walnut_monorepo");

    let packages = package_tree::make(&project_root);
    let rescript_version = build::get_version(&project_root);

    let modules =
        build::parse_and_get_dependencies(rescript_version, &project_root, packages.to_owned());

    println!("FINISH CONVERSION TO AST");

    // let all_modules = modules
    //     .keys()
    //     .map(|key| key.to_owned())
    //     .collect::<AHashSet<String>>();

    let mut compiled_modules = AHashSet::<String>::new();
    loop {
        dbg!("COMPILE PASS");
        let mut compiled_count = 0;
        modules
            // .iter()
            .par_iter()
            .map(|(module_name, module)| {
                if module.deps.is_subset(&compiled_modules)
                    && !compiled_modules.contains(module_name)
                {
                    match module.source_type.to_owned() {
                        build::SourceType::MlMap => Some(module_name.to_owned()),
                        build::SourceType::SourceFile => {
                            // compile interface first
                            match module.asti_path.to_owned() {
                                Some(asti_path) => {
                                    build::compile_file(
                                        &module.package.name,
                                        &asti_path,
                                        module,
                                        &project_root,
                                        true,
                                    );
                                }
                                _ => {
                                    ();
                                }
                            }

                            build::compile_file(
                                &module.package.name,
                                &module.ast_path.to_owned().unwrap(),
                                module,
                                &project_root,
                                false,
                            );
                            Some(module_name.to_owned())
                        }
                    }
                } else if !compiled_modules.contains(module_name) {
                    None
                } else {
                    None
                }
            })
            .collect::<Vec<Option<String>>>()
            .iter()
            .for_each(|module_name| {
                module_name.iter().for_each(|name| {
                    compiled_count += 1;
                    compiled_modules.insert(name.to_string());
                })
            });

        if compiled_count == 0 {
            //dbg!(all_modules
            //.difference(&compiled_modules)
            //.collect::<Vec<&String>>());
            dbg!("No incremental compile -- breaking");
            break;
        };
    }
}
