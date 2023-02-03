pub mod bsconfig;
pub mod build;
pub mod helpers;
pub mod package_tree;
pub mod structure_hashmap;
pub mod watcher;

fn main() {
    let folder = "walnut_monorepo";

    let package_tree = package_tree::make(&folder);
    let rescript_version = build::get_version(&folder);
    let source_files = build::generate_asts(rescript_version, &folder, package_tree);

    dbg!(source_files.len());
}
