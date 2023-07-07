use regex::Regex;
pub mod bsconfig;
pub mod build;
pub mod build_types;
pub mod clean;
pub mod helpers;
pub mod logs;
pub mod package_tree;
pub mod queue;
pub mod watcher;

fn main() {
    env_logger::init();
    let command = std::env::args().nth(1).unwrap_or("build".to_string());
    let folder = std::env::args().nth(2).unwrap_or(".".to_string());
    let filter = std::env::args()
        .nth(3)
        .map(|filter| Regex::new(filter.as_ref()).expect("Could not parse regex"));

    match command.as_str() {
        "clean" => {
            build::clean(&folder);
        }
        "build" => {
            match build::build(&filter, &folder) {
                Err(()) => std::process::exit(1),
                Ok(_) => std::process::exit(0),
            };
        }
        "watch" => {
            let _modules = build::build(&filter, &folder);
            watcher::start(&filter, &folder);
        }
        _ => println!("Not a valid build command"),
    }
}
