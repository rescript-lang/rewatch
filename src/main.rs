pub mod bsconfig;
pub mod build;
pub mod grouplike;
pub mod helpers;
pub mod package_tree;
pub mod structure_hashmap;
pub mod watcher;

fn main() {
    env_logger::init();
    let command = std::env::args().nth(1).unwrap_or("build".to_string());
    let folder = std::env::args().nth(2).unwrap_or(".".to_string());
    match command.as_str() {
        "clean" => {
            build::clean(&folder);
        }
        "build" => {
            match build::build(&folder) {
                Err(()) => std::process::exit(1),
                Ok(_) => std::process::exit(0),
            };
        }
        "watch" => {
            let _modules = build::build(&folder);
            watcher::start(&folder);
        }
        _ => println!("Not a valid build command"),
    }
}
