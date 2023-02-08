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

    let mut compiled_modules = AHashSet::<String>::new();

    for (module, source_file) in modules.iter() {
        if source_file.ast_deps.is_subset(&compiled_modules) {
            match source_file.source_type.to_owned() {
                build::SourceType::MlMap => {
                    build::compile_mlmap(&source_file.package, module, &project_root)
                }
                build::SourceType::Implementation | build::SourceType::Interface => {
                    build::compile_file(
                        &get_package_path(&project_root, &source_file.package.name),
                        &get_node_modules_path(&project_root),
                        source_file,
                        source_file.source_type == build::SourceType::Interface,
                    )
                }
            }
            let _ = compiled_modules.insert(module.to_owned());
        }
    }
}
