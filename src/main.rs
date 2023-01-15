pub mod bsconfig;
pub mod structure;
pub mod structure_hashmap;
pub mod watcher;
use ahash::{AHashMap, AHashSet};
use bsconfig::*;
use std::fs;

#[derive(Debug)]
pub struct Package {
    pub name: String,
    pub parent: Option<String>,
    pub bsconfig: bsconfig::T,
    pub source_folders: AHashSet<(String, bsconfig::QualifiedSource)>,
}

fn read_files(dir: &String, source: &QualifiedSource) -> AHashMap<String, fs::Metadata> {
    let mut map: AHashMap<String, fs::Metadata> = AHashMap::new();

    let (recurse, type_) = match source {
        QualifiedSource {
            subdirs: Some(Subdirs::Recurse(subdirs)),
            type_,
            ..
        } => (subdirs.to_owned(), type_),
        QualifiedSource { type_, .. } => (false, type_),
    };

    let structure = structure_hashmap::read_structure(dir, "res", recurse);

    match structure {
        Ok(files) => map.extend(files),
        Err(_e) if type_ == &Some("dev".to_string()) => {
            println!("Could not read folder: {dir}... Probably ok as type is dev")
        }
        Err(_e) => println!("Could not read folder: {dir}..."),
    }

    map
}

fn get_source(dir: &String, source: Source) -> AHashSet<(String, bsconfig::QualifiedSource)> {
    let mut source_folders: AHashSet<(String, bsconfig::QualifiedSource)> = AHashSet::new();

    let (root, subdirs, full_recursive) = match source.to_owned() {
        Source::Shorthand(dir)
        | Source::Qualified(QualifiedSource {
            dir, subdirs: None, ..
        }) => (dir, None, false),
        Source::Qualified(QualifiedSource {
            dir,
            subdirs: Some(Subdirs::Recurse(recurse)),
            ..
        }) => (dir, None, recurse),
        Source::Qualified(QualifiedSource {
            dir,
            subdirs: Some(Subdirs::Qualified(subdirs)),
            ..
        }) => (dir, Some(subdirs), false),
    };

    let full_path = dir.to_owned() + "/" + &root;
    source_folders.insert((
        full_path.to_owned(),
        bsconfig::to_qualified_without_children(&source),
    ));

    if !full_recursive {
        subdirs
            .unwrap_or(vec![])
            .iter()
            .for_each(|subdir| source_folders.extend(get_source(&full_path, subdir.to_owned())))
    }

    source_folders
}

fn build(dir: String, parent: Option<String>) -> AHashMap<String, Package> {
    let mut children: AHashMap<String, Package> = AHashMap::new();

    let bsconfig = bsconfig::read(dir.to_owned() + "/bsconfig.json");

    let source_folders = match bsconfig.sources.to_owned() {
        bsconfig::OneOrMore::Single(source) => get_source(&dir, source),
        bsconfig::OneOrMore::Multiple(sources) => {
            let mut source_folders: AHashSet<(String, bsconfig::QualifiedSource)> = AHashSet::new();
            sources
                .iter()
                .for_each(|source| source_folders.extend(get_source(&dir, source.to_owned())));
            source_folders
        }
    };

    /* At this point in time we may have started encountering elements multiple times as there is
     * no deduplication on the package level so far. Once we return this flat list of packages, do
     * have this deduplication. From that point on, we can add the source files for every single
     * one as that is an expensive operation IO wise and we don't want to duplicate that.*/
    children.insert(
        dir.to_owned(),
        Package {
            name: bsconfig.name.to_owned(),
            parent,
            bsconfig: bsconfig.to_owned(),
            source_folders,
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
    /* By Extending, we should eventually be able to parallalize */
    let mut map: AHashMap<String, Package> = AHashMap::new();
    map.extend(build("walnut_monorepo".to_string(), None));

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
            map.extend(read_files(dir, source));
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
