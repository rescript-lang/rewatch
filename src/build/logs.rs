use crate::build::packages::Package;
use crate::helpers;
use ahash::AHashMap;
use log::error;
use rayon::prelude::*;
use regex::Regex;
use std::fs::File;
use std::io::prelude::*;

use super::packages;

enum Location {
    Bs,
    Ocaml,
}

fn get_log_file_path(subfolder: Location, package: &packages::Package) -> String {
    let build_folder = match subfolder {
        Location::Bs => package.get_bs_build_path(),
        Location::Ocaml => package.get_build_path(),
    };

    build_folder.to_owned() + "/.compiler.log"
}

fn escape_colours(str: &str) -> String {
    let re = Regex::new(r"[\u001b\u009b]\[[()#;?]*(?:[0-9]{1,4}(?:;[0-9]{0,4})*)?[0-9A-ORZcf-nqry=><]")
        .expect("Could not create regex");
    re.replace_all(str, "").to_string()
}

fn write_to_log_file(mut file: File, package_name: &str, content: &str) {
    let res = file.write(escape_colours(content).as_bytes()).map_err(|e| {
        error!(
            "Could not create compiler log file. {}. \n{:?}",
            &package_name, &e
        );
    });

    match res {
        Ok(_) => {}
        Err(e) => error!(
            "Could not create compiler log file. {}. \n{:?}",
            &package_name, &e
        ),
    }
}

pub fn initialize(packages: &AHashMap<String, Package>) {
    packages.par_iter().for_each(|(name, package)| {
        let _ = File::create(get_log_file_path(Location::Bs, package))
            .map(|file| write_to_log_file(file, &name, &format!("#Start({})\n", helpers::get_system_time())))
            .expect(&("Cannot create compiler log for package ".to_owned() + name));
    })
}

pub fn append(package: &packages::Package, str: &str) {
    File::options()
        .append(true)
        .open(get_log_file_path(Location::Bs, package))
        .map(|file| write_to_log_file(file, &package.name, str))
        .expect(&("Cannot write compilerlog: ".to_owned() + &get_log_file_path(Location::Bs, package)));
}

pub fn finalize(packages: &AHashMap<String, Package>) {
    packages.par_iter().for_each(|(name, package)| {
        let _ = File::options()
            .append(true)
            .open(get_log_file_path(Location::Bs, package))
            .map(|file| write_to_log_file(file, &name, &format!("#Done({})\n", helpers::get_system_time())));

        let _ = std::fs::copy(
            get_log_file_path(Location::Bs, package),
            get_log_file_path(Location::Ocaml, package),
        );
    })
}
