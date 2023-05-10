use crate::helpers;
use crate::package_tree::Package;
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

fn get_log_file_path(subfolder: Location, name: &str) -> String {
    let subfolder_str = match subfolder {
        Location::Bs => "bs",
        Location::Ocaml => "ocaml",
    };
    name.to_owned() + "/lib/" + subfolder_str + "/.compiler.log"
}

fn escape_colours(str: &str) -> String {
    let re =
        Regex::new(r"[\u001b\u009b]\[[()#;?]*(?:[0-9]{1,4}(?:;[0-9]{0,4})*)?[0-9A-ORZcf-nqry=><]")
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
    packages.par_iter().for_each(|(name, _)| {
        let _ = File::create(get_log_file_path(Location::Bs, name)).map(|file| {
            write_to_log_file(
                file,
                &name,
                &format!("#Start({})\n", helpers::get_system_time()),
            )
        });
    })
}

pub fn append(name: &str, str: &str) {
    let _ = File::options()
        .append(true)
        .open(get_log_file_path(Location::Bs, name))
        .map(|file| write_to_log_file(file, &name, str));
}

pub fn finalize(packages: &AHashMap<String, Package>) {
    packages.par_iter().for_each(|(name, _)| {
        let _ = File::options()
            .append(true)
            .open(get_log_file_path(Location::Bs, name))
            .map(|file| {
                write_to_log_file(
                    file,
                    &name,
                    &format!("#Done({})\n", helpers::get_system_time()),
                )
            });

        let _ = std::fs::copy(get_log_file_path(Location::Bs, name), get_log_file_path(Location::Ocaml, name));
    })
}
