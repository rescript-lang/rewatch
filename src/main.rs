pub mod bsconfig;
pub mod structure;
pub mod structure_hashmap;
pub mod watcher;
use ahash::{AHashMap, AHashSet};
use bsconfig::*;
use std::fs;

/* this may be needed when reading */
//let source_is_dev = match source {
//Source::Qualified(QualifiedSource { type_, .. }) => type_ == Some("dev".to_string()),
//_ => false,
//};

#[derive(Debug)]
pub struct Build {
    pub parent: Option<String>,
    pub bsconfig: bsconfig::T,
    pub source_folders: AHashSet<String>,
    //pub sources: AHashMap<String, fs::Metadata>,
}

fn get_source(dir: &String, source: Source) -> AHashSet<String> {
    let mut source_folders: AHashSet<String> = AHashSet::new();

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

    source_folders.insert(dir.to_owned() + "/" + &root);

    if !full_recursive {
        subdirs
            .unwrap_or(vec![])
            .iter()
            .for_each(|subdir| source_folders.extend(get_source(&root, subdir.to_owned())))
    }

    source_folders
}

fn build(dir: String, parent: Option<String>) -> AHashMap<String, Build> {
    let mut children: AHashMap<String, Build> = AHashMap::new();

    let bsconfig = bsconfig::read(dir.to_owned() + "/bsconfig.json");

    let source_folders = match bsconfig.sources.to_owned() {
        bsconfig::OneOrMore::Single(source) => get_source(&dir, source),
        bsconfig::OneOrMore::Multiple(sources) => {
            let mut source_folders: AHashSet<String> = AHashSet::new();
            sources
                .iter()
                .for_each(|source| source_folders.extend(get_source(&dir, source.to_owned())));
            source_folders
        }
    };

    dbg!(&source_folders);

    children.insert(
        dir.to_owned(),
        Build {
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
    //let structure = structure_hashmap::read_structure("walnut_monorepo", "res");

    //match structure {
    //Ok(s) => println!("{:?}", s),
    //Err(_) => println!("Could not read fs"),
    //}

    /* By Extending, we should eventually be able to parallalize */
    let mut map: AHashMap<String, Build> = AHashMap::new();
    map.extend(build("walnut_monorepo".to_string(), None));

    //map.iter().for_each(|(key, value)| {
    //dbg!(key, &value.parent, &value.bsconfig.bs_dependencies);
    //});

    futures::executor::block_on(async {
        if let Err(e) = watcher::async_watch("walnut_monorepo").await {
            println!("error: {:?}", e)
        }
    });
}
