pub mod bsconfig;
pub mod package_tree;
pub mod build;
pub mod structure_hashmap;
pub mod watcher;


fn main() {
    let folder = "walnut_monorepo";
    let package_tree = package_tree::make(&folder);
    let source_files = build::get_source_files(package_tree);
    dbg!(&source_files);
    dbg!(source_files.len());
}
