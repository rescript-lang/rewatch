pub mod bsconfig;
pub mod build;
pub mod structure_hashmap;
pub mod watcher;
use ahash::AHashMap;
use std::fs;

fn main() {
    /* By Extending, we should eventually be able to parallalize */
    let mut map: AHashMap<String, build::Package> = AHashMap::new();
    map.extend(build::make("walnut_monorepo".to_string(), None));

    for (_key, value) in map.iter_mut() {
        /* We may want to directly build a reverse-lookup from filename -> package while we do this */
        let mut map: AHashMap<String, fs::Metadata> = AHashMap::new();
        value.source_folders.iter().for_each(|(dir, source)| {
            map.extend(build::read_files(dir, source));
        });

        value.source_files = Some(map);
    }

    for (_key, value) in map {
        dbg!(&value.bsconfig.name, &value.source_files);
    }

    futures::executor::block_on(async {
        if let Err(e) = watcher::async_watch("walnut_monorepo").await {
            println!("error: {:?}", e)
        }
    });
}
