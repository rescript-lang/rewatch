pub mod bsconfig;
pub mod build;
pub mod helpers;
pub mod package_tree;
pub mod structure_hashmap;
pub mod watcher;
use ahash::AHashSet;
use helpers::*;

fn main() {
    let project_root = helpers::get_abs_path("walnut_monorepo");

    let packages = package_tree::make(&project_root);
    let rescript_version = build::get_version(&project_root);
    let modules =
        build::parse_and_get_dependencies(rescript_version, &project_root, packages.to_owned());

    println!("FINISH CONVERSION TO AST");

    let all_modules = modules
        .keys()
        .map(|key| key.to_owned())
        .collect::<AHashSet<String>>();

    let mut compiled_modules = AHashSet::<String>::new();
    loop {
        dbg!("COMPILE PASS");
        let mut compiled_count = 0;
        for (module_name, module) in modules.iter() {
            if module.deps.is_subset(&compiled_modules) && !compiled_modules.contains(module_name) {
                compiled_count += 1;
                match module.source_type.to_owned() {
                    build::SourceType::MlMap => {
                        // build::compile_mlmap(&module.package, module, &project_root)
                    }
                    build::SourceType::SourceFile => {
                        // compile interface first
                        match module.asti_path.to_owned() {
                            Some(asti_path) => {
                                build::compile_file(
                                    &get_package_path(&project_root, &module.package.name),
                                    &get_node_modules_path(&project_root),
                                    &asti_path,
                                    module,
                                    true,
                                );
                            }
                            _ => (),
                        }

                        build::compile_file(
                            &get_package_path(&project_root, &module.package.name),
                            &get_node_modules_path(&project_root),
                            &module.ast_path.to_owned().unwrap(),
                            module,
                            false,
                        );
                        let _ = compiled_modules.insert(module_name.to_owned());
                    }
                }
            } else if !compiled_modules.contains(module_name) {
                dbg!(format!("Still uncompiled deps for: {}", module_name));
                dbg!(module
                    .deps
                    .difference(&compiled_modules)
                    .collect::<Vec<&String>>());
            }
        }
        if compiled_count == 0 {
            dbg!(all_modules
                .difference(&compiled_modules)
                .collect::<Vec<&String>>());
            dbg!("No incremental compile -- breaking");
            break;
        };
    }
}
