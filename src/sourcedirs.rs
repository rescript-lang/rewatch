use crate::build::build_types::BuildState;
use crate::build::packages::Package;
use ahash::{AHashMap, AHashSet};
use rayon::prelude::*;
use serde::Serialize;
use serde_json::json;
use std::fs::File;
use std::io::prelude::*;
use std::path::PathBuf;

type Dir = String;
type PackageName = String;
type AbsolutePath = String;
type Pkg = (PackageName, AbsolutePath);

#[derive(Serialize, Debug, Clone, PartialEq, Hash)]
pub struct SourceDirs<'a> {
    pub dirs: &'a Vec<Dir>,
    pub pkgs: &'a Vec<Pkg>,
    pub generated: &'a Vec<String>,
}

fn package_to_dirs<'a>(package: &'a Package, root_package_path: &String) -> AHashSet<Dir> {
    let relative_path = PathBuf::from(&package.path)
        .strip_prefix(PathBuf::from(&root_package_path))
        .unwrap()
        .to_string_lossy()
        .to_string();

    package
        .dirs
        .as_ref()
        .unwrap_or(&AHashSet::new())
        .iter()
        .filter_map(|path| path.to_str().map(|path| format!("{relative_path}/{path}")))
        .collect::<AHashSet<String>>()
}

fn deps_to_pkgs<'a>(
    packages: &'a AHashMap<String, Package>,
    dependencies: &'a Option<Vec<String>>,
) -> AHashSet<Pkg> {
    dependencies
        .as_ref()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|name| {
            packages
                .get(name)
                .map(|package| (name.to_owned(), package.path.to_owned()))
        })
        .collect::<AHashSet<Pkg>>()
}

fn write_sourcedirs_files(path: String, source_dirs: &SourceDirs) -> Result<usize, std::io::Error> {
    let mut source_dirs_json = File::create(path + "/.sourcedirs.json")?;
    source_dirs_json.write(json!(source_dirs).to_string().as_bytes())
}

pub fn print(buildstate: &BuildState) {
    // Find Root Package
    let (_name, root_package) = buildstate
        .packages
        .iter()
        .find(|(_name, package)| package.is_root)
        .expect("Could not find root package");

    // Take all packages apart from the root package
    let (dirs, pkgs): (Vec<AHashSet<Dir>>, Vec<AHashMap<PackageName, AbsolutePath>>) = buildstate
        .packages
        .par_iter()
        .filter(|(_name, package)| !package.is_root)
        .map(|(_name, package)| {
            // Extract Directories
            let dirs = package_to_dirs(&package, &root_package.path);

            // Extract Pkgs
            let pkgs = [
                &package.bsconfig.pinned_dependencies,
                &package.bsconfig.bs_dependencies,
                &package.bsconfig.bs_dev_dependencies,
            ]
            .into_iter()
            .map(|dependencies| deps_to_pkgs(&buildstate.packages, dependencies));

            // Write sourcedirs.json
            write_sourcedirs_files(
                package.get_bs_build_path(),
                &SourceDirs {
                    dirs: &dirs.clone().into_iter().collect::<Vec<Dir>>(),
                    pkgs: &pkgs.clone().flatten().collect::<Vec<Pkg>>(),
                    generated: &vec![],
                },
            )
            .expect("Could not write sourcedirs.json");

            (
                dirs,
                pkgs.flatten().collect::<AHashMap<PackageName, AbsolutePath>>(),
            )
        })
        .unzip();

    let mut merged_dirs: AHashSet<Dir> = AHashSet::new();
    let mut merged_pkgs: AHashMap<PackageName, AbsolutePath> = AHashMap::new();

    dirs.into_iter().for_each(|dir_set| merged_dirs.extend(dir_set));
    pkgs.into_iter().for_each(|pkg_set| merged_pkgs.extend(pkg_set));

    // Write sourcedirs.json
    write_sourcedirs_files(
        root_package.get_bs_build_path(),
        &SourceDirs {
            dirs: &merged_dirs.into_iter().collect::<Vec<Dir>>(),
            pkgs: &merged_pkgs.into_iter().collect::<Vec<Pkg>>(),
            generated: &vec![],
        },
    )
    .expect("Could not write sourcedirs.json");
}
