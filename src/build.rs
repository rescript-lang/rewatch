use crate::bsconfig;
use crate::bsconfig::*;
use crate::structure_hashmap;
use ahash::{AHashMap, AHashSet};
use std::fs;

#[derive(Debug)]
pub struct Package {
    pub name: String,
    pub parent: Option<String>,
    pub bsconfig: bsconfig::T,
    pub source_folders: AHashSet<(String, bsconfig::QualifiedSource)>,
    pub source_files: Option<AHashMap<String, fs::Metadata>>,
}

/// # Read Files from a folder, possibly recursive
///
/// `read_files` is essentially a wrapper around `structure_hashmap::read_structure`, which read a
/// list of files in a folder to a hashmap of `string` / `fs::Metadata` (file metadata). Reason for
/// this wrapper is the recursiveness of the `bsconfig.json` subfolders. Some sources in bsconfig
/// can be specified as being fully recursive (`{ subdirs: true }`). This wrapper pulls out that
/// data from the config and pushes it forwards. Another thing is the 'type_', some files / folders
/// can be marked with the type 'dev'. Which means that they may not be around in the distributed
/// NPM package. The file reader allows for this, just warns when this happens.
pub fn read_files(dir: &String, source: &QualifiedSource) -> AHashMap<String, fs::Metadata> {
    let mut map: AHashMap<String, fs::Metadata> = AHashMap::new();

    let (recurse, type_) = match source {
        QualifiedSource {
            subdirs: Some(Subdirs::Recurse(subdirs)),
            type_,
            ..
        } => (subdirs.to_owned(), type_),
        QualifiedSource { type_, .. } => (false, type_),
    };

    let structure = structure_hashmap::read_folders(dir, recurse);

    match structure {
        Ok(files) => map.extend(files),
        Err(_e) if type_ == &Some("dev".to_string()) => {
            println!("Could not read folder: {dir}... Probably ok as type is dev")
        }
        Err(_e) => println!("Could not read folder: {dir}..."),
    }

    map
}

/// # Get Sources
///
/// Given a projects' root folder and a `bsconfig::Source`, this recursively creates all the
/// sources in a flat list. In the process, it removes the children, as they are being resolved
/// because of the recursiveness. So you get a flat list of files back, retaining the type_ and
/// wether it needs to recurse into all structures
///
/// TODO - Break out "QualifiedSource" -> "InternalSource" or something like that, for clarity
///
/// TODO - Make HashMap instead? For 0(1) access? Can't hurt?
fn get_sources(
    project_root: &str,
    source: Source,
) -> AHashSet<(String, bsconfig::QualifiedSource)> {
    let mut source_folders: AHashSet<(String, bsconfig::QualifiedSource)> = AHashSet::new();

    let (package_root, subdirs, full_recursive) = match source.to_owned() {
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

    let full_path = project_root.to_string() + "/" + &package_root;
    source_folders.insert((
        full_path.to_owned(),
        bsconfig::to_qualified_without_children(&source),
    ));

    if !full_recursive {
        subdirs
            .unwrap_or(vec![])
            .iter()
            .for_each(|subdir| source_folders.extend(get_sources(&full_path, subdir.to_owned())))
    }

    source_folders
}

/// # Make Package
///
/// Given a directory that includes a bsconfig file, read it, and recursively find all other
/// bsconfig files, and turn those into Packages as well.
///
/// TODO -> Make private / add public wrapper without parent. Hide implementation details.
pub fn make(root_dir: &str, parent: Option<String>) -> AHashMap<String, Package> {
    let mut children: AHashMap<String, Package> = AHashMap::new();

    let bsconfig = bsconfig::read(root_dir.to_string() + "/bsconfig.json");

    let source_folders = match bsconfig.sources.to_owned() {
        bsconfig::OneOrMore::Single(source) => get_sources(root_dir, source),
        bsconfig::OneOrMore::Multiple(sources) => {
            let mut source_folders: AHashSet<(String, bsconfig::QualifiedSource)> = AHashSet::new();
            sources
                .iter()
                .for_each(|source| source_folders.extend(get_sources(root_dir, source.to_owned())));
            source_folders
        }
    };

    /* At this point in time we may have started encountering elements multiple times as there is
     * no deduplication on the package level so far. Once we return this flat list of packages, do
     * have this deduplication. From that point on, we can add the source files for every single
     * one as that is an expensive operation IO wise and we don't want to duplicate that.*/
    children.insert(
        root_dir.to_string(),
        Package {
            name: bsconfig.name.to_owned(),
            parent,
            bsconfig: bsconfig.to_owned(),
            source_folders,
            source_files: None,
        },
    );

    bsconfig
        .bs_dependencies
        .to_owned()
        .unwrap_or(vec![])
        .iter()
        .for_each(|dep| {
            children.extend(make(
                // TODO - Fix constant
                &("walnut_monorepo/node_modules/".to_string() + &dep),
                Some(root_dir.to_string()),
            ))
        });

    children
}
