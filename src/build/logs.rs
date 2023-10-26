use crate::build::packages::Package;
use crate::helpers;
use ahash::AHashMap;
use log::error;
use rayon::prelude::*;
use regex::Regex;
use std::fs::File;
use std::io::prelude::*;

enum Location {
    Bs,
    Ocaml,
}

fn get_log_file_path(project_root: &str, subfolder: Location, name: &str, is_root: bool) -> String {
    let build_folder = match subfolder {
        Location::Bs => helpers::get_bs_build_path(project_root, name, is_root),
        Location::Ocaml => helpers::get_build_path(project_root, name, is_root),
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

pub fn initialize(project_root: &str, packages: &AHashMap<String, Package>) {
    packages.par_iter().for_each(|(name, package)| {
        File::create(get_log_file_path(
            project_root,
            Location::Bs,
            name,
            package.is_root,
        ))
        .map(|file| write_to_log_file(file, name, &format!("#Start({})\n", helpers::get_system_time())))
        .expect(&("Cannot create compiler log for package ".to_owned() + name));
    })
}

pub fn append(project_root: &str, is_root: bool, name: &str, str: &str) {
    File::options()
        .append(true)
        .open(get_log_file_path(project_root, Location::Bs, name, is_root))
        .map(|file| write_to_log_file(file, name, str))
        .expect(
            &("Cannot write compilerlog: ".to_owned()
                + &get_log_file_path(project_root, Location::Bs, name, is_root)),
        );
}

pub fn finalize(project_root: &str, packages: &AHashMap<String, Package>) {
    packages.par_iter().for_each(|(name, package)| {
        let _ = File::options()
            .append(true)
            .open(get_log_file_path(
                project_root,
                Location::Bs,
                name,
                package.is_root,
            ))
            .map(|file| write_to_log_file(file, name, &format!("#Done({})\n", helpers::get_system_time())));

        let _ = std::fs::copy(
            get_log_file_path(project_root, Location::Bs, name, package.is_root),
            get_log_file_path(project_root, Location::Ocaml, name, package.is_root),
        );
    })
}
