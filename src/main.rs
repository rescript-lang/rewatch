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

    let mut source_files: AHashMap<String, fs::Metadata> = AHashMap::new();
    /* We should do this in order of the tree. Start with the leaves, then walk up. Then, within
     * there, get the dependencies for every file (ie, map<filename, [deps]>), then resort the map
     * at the end, insert all the deps of the files before the filenames. Probably in some ordered
     * set like structure to we can remove duplicate files. As long as we sort the dependencies
     * correctly first, the ones without dependencies should sort to the top automatically. */
    map.iter().for_each(|(_key, value)| {
        /* We may want to directly build a reverse-lookup from filename -> package while we do this */
        let mut map: AHashMap<String, fs::Metadata> = AHashMap::new();
        value.source_folders.iter().for_each(|(dir, source)| {
            map.extend(build::read_files(dir, source));
        });
        source_files.extend(map)
    });

    dbg!(source_files.keys());

    futures::executor::block_on(async {
        if let Err(e) = watcher::async_watch("walnut_monorepo").await {
            println!("error: {:?}", e)
        }
    });
}
