use crate::build::build_types::BuildState;
use crate::build::packages::Package;
use ahash::{AHashMap, AHashSet};
use rayon::prelude::*;
use serde::Serialize;
use serde_json::json;
use std::fs::File;
use std::io::prelude::*;

type Dir = String;
type PackageName = String;
type AbsolutePath = String;
type PackagePath = String;

#[derive(Serialize, Debug, Clone, PartialEq, Hash)]
pub struct SourceDirs {
    pub dirs: Vec<Dir>,
    pub pkgs: Vec<(PackageName, AbsolutePath)>,
    pub generated: Vec<String>,
}

pub fn print(buildstate: &BuildState) {
    let (_name, package) = buildstate
        .packages
        .iter()
        .find(|(_name, package)| package.is_root)
        .expect("Could not find root package");

    // First do all child packages
    let child_packages = buildstate
        .packages
        .par_iter()
        .filter(|(_name, package)| !package.is_root)
        .map(|(_name, package)| {
            let path = package.get_bs_build_path();

            let dirs = package
                .dirs
                .to_owned()
                .unwrap_or(AHashSet::new())
                .iter()
                .filter_map(|path| path.to_str().map(|x| x.to_string()))
                .collect::<AHashSet<String>>();

            fn deps_to_pkgs<'a>(
                packages: &'a AHashMap<String, Package>,
                xs: &'a Option<Vec<String>>,
            ) -> AHashSet<(String, PackagePath)> {
                xs.as_ref()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|name| {
                        packages
                            .get(&name.to_owned())
                            .map(|package| (name.clone(), package.path.clone()))
                    })
                    .collect::<AHashSet<(String, PackagePath)>>()
            }

            let pinned_dependencies =
                deps_to_pkgs(&buildstate.packages, &package.bsconfig.pinned_dependencies);
            let bs_dependencies = deps_to_pkgs(&buildstate.packages, &package.bsconfig.bs_dependencies);
            let bs_dev_dependencies =
                deps_to_pkgs(&buildstate.packages, &package.bsconfig.bs_dev_dependencies);

            let mut pkgs = AHashMap::new();
            pkgs.extend(pinned_dependencies);
            pkgs.extend(bs_dependencies);
            pkgs.extend(bs_dev_dependencies);

            let name = path + "/.sourcedirs.json";
            let _ = File::create(&name).map(|mut file| {
                let source_files = SourceDirs {
                    dirs: dirs.clone().into_iter().collect::<Vec<Dir>>(),
                    pkgs: pkgs
                        .clone()
                        .into_iter()
                        .collect::<Vec<(PackageName, AbsolutePath)>>(),
                    generated: vec![],
                };

                file.write(json!(source_files).to_string().as_bytes())
            });
            let _ = std::fs::copy(package.get_bs_build_path(), package.get_build_path());

            (&package.path, dirs, pkgs)
        })
        .collect::<Vec<(
            &PackagePath,
            AHashSet<String>,
            AHashMap<PackageName, AbsolutePath>,
        )>>();

    let mut all_dirs = AHashSet::new();
    let mut all_pkgs: AHashMap<PackageName, AbsolutePath> = AHashMap::new();

    child_packages.iter().for_each(|(package_path, dirs, pkgs)| {
        dirs.iter().for_each(|dir| {
            all_dirs.insert(format!("{package_path}/{dir}"));
        });

        all_pkgs.extend(pkgs.to_owned());
    });

    let path = package.get_bs_build_path();
    let name = path + "/.sourcedirs.json";
    let _ = File::create(name.clone()).map(|mut file| {
        let all_source_files = SourceDirs {
            dirs: all_dirs.into_iter().collect::<Vec<String>>(),
            pkgs: all_pkgs.into_iter().collect::<Vec<(PackageName, AbsolutePath)>>(),
            generated: vec![],
        };
        file.write(json!(all_source_files).to_string().as_bytes())
    });

    let _ = std::fs::copy(package.get_bs_build_path(), package.get_build_path());
}
