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
    let source_files = build::get_dependencies(rescript_version, &folder, package_tree);

    //source_files
        //.iter()
        //.filter(|(_file, source)| source.ast_deps.len() == 0)
        //.for_each(|(_file, source)| {
            ////dbg!(file);
            //let pkg_path_abs = folder.to_owned() + "/node_modules/" + &source.bsconfig.name;
            //let abs_node_modules_path =
                //helpers::get_abs_path(&(folder.to_owned() + "/node_modules"));

            //build::compile_file(&pkg_path_abs, &abs_node_modules_path, source);
        //});
}
