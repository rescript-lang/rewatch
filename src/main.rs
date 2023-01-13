pub mod bsconfig;
pub mod structure;
pub mod watcher;

fn main() {
    let structure = structure::read_structure("walnut_monorepo");

    match structure {
        Ok(s) => println!("{}", s),
        Err(_) => println!("Could not read fs"),
    }
    let root_bs_config = bsconfig::read("walnut_monorepo/bsconfig.json".to_string());
    let _ = root_bs_config
        .pinned_dependencies
        .unwrap_or(vec![])
        .iter()
        .map(|dep| bsconfig::read("walnut_monorepo/node_modules/".to_string() + dep + "/bsconfig.json"))
        .for_each(|config| println!("{config:?}"));

    futures::executor::block_on(async {
        if let Err(e) = watcher::async_watch("walnut_monorepo").await {
            println!("error: {:?}", e)
        }
    });
}
