pub mod bsconfig;
pub mod structure;
pub mod structure_hashmap;
pub mod watcher;
use ahash::AHashMap;
use std::fs;

#[derive(Debug)]
pub struct Build {
    pub parent: Option<String>,
    pub bsconfig: bsconfig::T,
    //pub structure: AHashMap<String, fs::Metadata>,
}

fn build(dir: String, parent: Option<String>) -> AHashMap<String, Build> {
    let mut children: AHashMap<String, Build> = AHashMap::new();

    let bsconfig = bsconfig::read(dir.to_owned() + "/bsconfig.json");

    children.insert(
        dir.to_owned(),
        Build {
            parent,
            bsconfig: bsconfig.to_owned(),
        },
    );

    bsconfig
        .bs_dependencies
        .to_owned()
        .unwrap_or(vec![])
        .iter()
        .for_each(|dep| {
            children.extend(build(
                "walnut_monorepo/node_modules/".to_string() + &dep,
                Some(dir.to_owned()),
            ))
        });

    children
}

fn main() {
    //let structure = structure_hashmap::read_structure("walnut_monorepo", "res");

    //match structure {
    //Ok(s) => println!("{:?}", s),
    //Err(_) => println!("Could not read fs"),
    //}

    /* By Extending, we should eventually be able to parallalize */
    let mut map: AHashMap<String, Build> = AHashMap::new();
    map.extend(build("walnut_monorepo".to_string(), None));

    map.iter().for_each(|(key, value)| {
        dbg!(key, &value.bsconfig.bs_dependencies);
    });

    futures::executor::block_on(async {
        if let Err(e) = watcher::async_watch("walnut_monorepo").await {
            println!("error: {:?}", e)
        }
    });
}
