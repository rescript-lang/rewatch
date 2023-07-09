use crate::bsconfig;
use crate::helpers;
use ahash::{AHashMap, AHashSet};
use convert_case::{Case, Casing};
use rayon::prelude::*;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::{error, fs};

#[derive(Debug, Clone)]
pub struct SourceFileMeta {
    pub modified: SystemTime,
}

#[derive(Debug, Clone)]
pub enum Namespace {
    Namespace(String),
    NamespaceWithEntry { namespace: String, entry: String },
    NoNamespace,
}

impl Namespace {
    pub fn to_suffix(&self) -> Option<String> {
        match self {
            Namespace::Namespace(namespace) => Some(namespace.to_string()),
            Namespace::NamespaceWithEntry {
                namespace,
                entry: _,
            } => Some("@".to_string() + namespace),
            Namespace::NoNamespace => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    pub bsconfig: bsconfig::T,
    pub source_folders: AHashSet<bsconfig::PackageSource>,
    // these are the relative file paths (relative to the package root)
    pub source_files: Option<AHashMap<String, SourceFileMeta>>,
    pub namespace: Namespace,
    pub modules: Option<AHashSet<String>>,
    // canonicalized dir of the package
    pub package_dir: String,
    pub dirs: Option<AHashSet<PathBuf>>,
    pub is_pinned_dep: bool,
}

impl PartialEq for Package {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
impl Eq for Package {}
impl Hash for Package {
    fn hash<H: Hasher>(&self, _state: &mut H) {
        blake3::hash(&self.name.as_bytes());
    }
}

fn matches_filter(filter: &Option<regex::Regex>, path: &str) -> bool {
    match filter {
        Some(filter) => filter.is_match(path),
        None => true,
    }
}

pub fn read_folders(
    filter: &Option<regex::Regex>,
    package_dir: &Path,
    path: &Path,
    recurse: bool,
) -> Result<AHashMap<String, SourceFileMeta>, Box<dyn error::Error>> {
    let mut map: AHashMap<String, SourceFileMeta> = AHashMap::new();
    let path_buf = PathBuf::from(path);
    let meta = fs::metadata(package_dir.join(&path));
    let path_with_meta = meta.map(|meta| {
        (
            path.to_owned(),
            SourceFileMeta {
                modified: meta.modified().unwrap(),
            },
        )
    });

    for entry in fs::read_dir(package_dir.join(&path_buf))? {
        let entry_path_buf = entry.map(|entry| entry.path())?;
        let metadata = fs::metadata(&entry_path_buf)?;
        let name = entry_path_buf
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        let path_ext = entry_path_buf.extension().and_then(|x| x.to_str());
        let new_path = path_buf.join(&name);
        if metadata.file_type().is_dir() && recurse {
            match read_folders(&filter, package_dir, &new_path, recurse) {
                Ok(s) => map.extend(s),
                Err(e) => println!("Error reading directory: {}", e),
            }
        }

        match path_ext {
            Some(extension) if helpers::is_source_file(extension) => match path_with_meta {
                Ok((ref path, _)) if matches_filter(filter, &name) => {
                    let mut path = path.to_owned();
                    path.push(&name);
                    map.insert(
                        path.to_string_lossy().to_string(),
                        SourceFileMeta {
                            modified: metadata.modified().unwrap(),
                        },
                    );
                }

                Ok(_) => println!("Filtered: {:?}", name),
                Err(ref e) => println!("Error reading directory: {}", e),
            },
            _ => (),
        }
    }

    Ok(map)
}

/// Given a projects' root folder and a `bsconfig::Source`, this recursively creates all the
/// sources in a flat list. In the process, it removes the children, as they are being resolved
/// because of the recursiveness. So you get a flat list of files back, retaining the type_ and
/// wether it needs to recurse into all structures
fn get_source_dirs(
    source: bsconfig::Source,
    sub_path: Option<PathBuf>,
) -> AHashSet<bsconfig::PackageSource> {
    let mut source_folders: AHashSet<bsconfig::PackageSource> = AHashSet::new();

    let (subdirs, full_recursive) = match source.to_owned() {
        bsconfig::Source::Shorthand(_)
        | bsconfig::Source::Qualified(bsconfig::PackageSource { subdirs: None, .. }) => {
            (None, false)
        }
        bsconfig::Source::Qualified(bsconfig::PackageSource {
            subdirs: Some(bsconfig::Subdirs::Recurse(recurse)),
            ..
        }) => (None, recurse),
        bsconfig::Source::Qualified(bsconfig::PackageSource {
            subdirs: Some(bsconfig::Subdirs::Qualified(subdirs)),
            ..
        }) => (Some(subdirs), false),
    };

    let source_folder = bsconfig::to_qualified_without_children(&source, sub_path.to_owned());
    source_folders.insert(source_folder.to_owned());

    if !full_recursive {
        let sub_path = Path::new(&source_folder.dir).to_path_buf();
        subdirs
            .unwrap_or(vec![])
            .par_iter()
            .map(|subdir| get_source_dirs(subdir.to_owned(), Some(sub_path.to_owned())))
            .collect::<Vec<AHashSet<bsconfig::PackageSource>>>()
            .into_iter()
            .for_each(|subdir| source_folders.extend(subdir))
    }

    source_folders
}

fn get_package_dir(package_name: &str, is_root: bool) -> String {
    if is_root {
        "".to_string()
    } else {
        helpers::get_relative_package_path(&package_name)
    }
}

fn read_bsconfig(package_dir: &str) -> bsconfig::T {
    if package_dir == "" {
        return bsconfig::read("bsconfig.json".to_string());
    }
    bsconfig::read(package_dir.to_string() + "/bsconfig.json")
}

/// # Make Package
/// Given a directory that includes a bsconfig file, read it, and recursively find all other
/// bsconfig files, and turn those into Packages as well.
fn build_package<'a>(
    map: &'a mut AHashMap<String, Package>,
    bsconfig: bsconfig::T,
    package_dir: &str,
    project_root: &str,
    is_pinned_dep: bool,
) -> &'a mut AHashMap<String, Package> {
    // let (package_dir, bsconfig) = read_bsconfig(package_name, project_root, is_root);
    let copied_bsconfig = bsconfig.to_owned();

    /* At this point in time we may have started encountering elements multiple times as there is
     * no deduplication on the package level so far. Once we return this flat list of packages, do
     * have this deduplication. From that point on, we can add the source files for every single
     * one as that is an expensive operation IO wise and we don't want to duplicate that.*/
    map.insert(copied_bsconfig.name.to_owned(), {
        let source_folders = match bsconfig.sources.to_owned() {
            bsconfig::OneOrMore::Single(source) => get_source_dirs(source, None),
            bsconfig::OneOrMore::Multiple(sources) => {
                let mut source_folders: AHashSet<bsconfig::PackageSource> = AHashSet::new();
                sources
                    .iter()
                    .map(|source| get_source_dirs(source.to_owned(), None))
                    .collect::<Vec<AHashSet<bsconfig::PackageSource>>>()
                    .into_iter()
                    .for_each(|source| source_folders.extend(source));
                source_folders
            }
        };

        let namespace_from_package = namespace_from_package_name(&bsconfig.name);
        Package {
            name: copied_bsconfig.name.to_owned(),
            bsconfig: copied_bsconfig,
            source_folders,
            source_files: None,
            namespace: match (bsconfig.namespace, bsconfig.namespace_entry) {
                (Some(bsconfig::Namespace::Bool(false)), _) => Namespace::NoNamespace,
                (None, _) => Namespace::NoNamespace,
                (Some(bsconfig::Namespace::Bool(true)), None) => {
                    Namespace::Namespace(namespace_from_package)
                }
                (Some(bsconfig::Namespace::Bool(true)), Some(entry)) => {
                    Namespace::NamespaceWithEntry {
                        namespace: namespace_from_package,
                        entry: entry,
                    }
                }
                (Some(bsconfig::Namespace::String(str)), None) => match str.as_str() {
                    "true" => Namespace::Namespace(namespace_from_package),
                    namespace if namespace.is_case(Case::UpperFlat) => {
                        Namespace::Namespace(namespace.to_string())
                    }
                    namespace => Namespace::Namespace(namespace.to_string().to_case(Case::Pascal)),
                },
                (Some(bsconfig::Namespace::String(str)), Some(entry)) => match str.as_str() {
                    "true" => Namespace::NamespaceWithEntry {
                        namespace: namespace_from_package,
                        entry,
                    },
                    namespace if namespace.is_case(Case::UpperFlat) => {
                        Namespace::NamespaceWithEntry {
                            namespace: namespace.to_string(),
                            entry: entry,
                        }
                    }
                    namespace => Namespace::NamespaceWithEntry {
                        namespace: namespace.to_string().to_case(Case::Pascal),
                        entry,
                    },
                },
            },
            modules: None,
            package_dir: package_dir.to_string(),
            dirs: None,
            is_pinned_dep: is_pinned_dep,
        }
    });

    bsconfig
        .bs_dependencies
        .to_owned()
        .unwrap_or(vec![])
        .iter()
        .filter_map(|package_name| {
            let package_dir = PathBuf::from(get_package_dir(package_name, false))
                .canonicalize()
                .expect("Could not canonicalize package dir")
                .to_string_lossy()
                .to_string();
            if !map.contains_key(package_name) {
                Some(package_dir)
            } else {
                None
            }
        })
        .collect::<Vec<String>>()
        // read all bsconfig files simultanously instead of blocking
        .par_iter()
        .map(|package_dir| (package_dir.to_owned(), read_bsconfig(package_dir)))
        .collect::<Vec<(String, bsconfig::T)>>()
        .iter()
        .fold(map, |map, (package_dir, child_bsconfig)| {
            build_package(
                map,
                child_bsconfig.to_owned(),
                &package_dir,
                &project_root,
                bsconfig
                    .pinned_dependencies
                    .as_ref()
                    .map(|p| p.contains(&child_bsconfig.name))
                    .unwrap_or(false),
            )
        })
}

/// `get_source_files` is essentially a wrapper around `read_structure`, which read a
/// list of files in a folder to a hashmap of `string` / `fs::Metadata` (file metadata). Reason for
/// this wrapper is the recursiveness of the `bsconfig.json` subfolders. Some sources in bsconfig
/// can be specified as being fully recursive (`{ subdirs: true }`). This wrapper pulls out that
/// data from the config and pushes it forwards. Another thing is the 'type_', some files / folders
/// can be marked with the type 'dev'. Which means that they may not be around in the distributed
/// NPM package. The file reader allows for this, just warns when this happens.
/// TODO -> Check wether we actually need the `fs::Metadata`
pub fn get_source_files(
    package_dir: &Path,
    filter: &Option<regex::Regex>,
    source: &bsconfig::PackageSource,
) -> AHashMap<String, SourceFileMeta> {
    let mut map: AHashMap<String, SourceFileMeta> = AHashMap::new();

    let (recurse, type_) = match source {
        bsconfig::PackageSource {
            subdirs: Some(bsconfig::Subdirs::Recurse(subdirs)),
            type_,
            ..
        } => (subdirs.to_owned(), type_),
        bsconfig::PackageSource { type_, .. } => (false, type_),
    };

    let path_dir = Path::new(&source.dir);
    // don't include dev sources for now
    if type_ != &Some("dev".to_string()) {
        match read_folders(&filter, package_dir, path_dir, recurse) {
            Ok(files) => map.extend(files),
            Err(_e) if type_ == &Some("dev".to_string()) => {
                println!(
                    "Could not read folder: {}... Probably ok as type is dev",
                    path_dir.to_string_lossy()
                )
            }
            Err(_e) => println!("Could not read folder: {}...", path_dir.to_string_lossy()),
        }
    }

    map
}

pub fn namespace_from_package_name(package_name: &str) -> String {
    package_name
        .to_owned()
        .replace("@", "")
        .replace("/", "_")
        .to_case(Case::Pascal)
}

/// This takes the tree of packages, and finds all the source files for each, adding them to the
/// respective packages.
fn extend_with_children(
    filter: &Option<regex::Regex>,
    mut build: AHashMap<String, Package>,
) -> AHashMap<String, Package> {
    for (_key, value) in build.iter_mut() {
        let mut map: AHashMap<String, SourceFileMeta> = AHashMap::new();
        value
            .source_folders
            .par_iter()
            .map(|source| get_source_files(Path::new(&value.package_dir), &filter, source))
            .collect::<Vec<AHashMap<String, SourceFileMeta>>>()
            .into_iter()
            .for_each(|source| map.extend(source));

        let mut modules = AHashSet::from_iter(
            map.keys()
                .map(|key| helpers::file_path_to_module_name(key, &value.namespace)),
        );
        match value.namespace.to_owned() {
            Namespace::Namespace(namespace) => {
                let _ = modules.insert(namespace);
            }
            Namespace::NamespaceWithEntry {
                namespace,
                entry: _,
            } => {
                let _ = modules.insert("@".to_string() + &namespace);
            }
            Namespace::NoNamespace => (),
        }
        value.modules = Some(modules);
        let mut dirs = AHashSet::new();
        map.keys().for_each(|path| {
            let dir = std::path::Path::new(&path).parent().unwrap();
            dirs.insert(dir.to_owned());
        });
        value.dirs = Some(dirs);
        value.source_files = Some(map);
    }
    build
}

/// Make turns a folder, that should contain a bsconfig, into a tree of Packages.
/// It does so in two steps:
/// 1. Get all the packages parsed, and take all the source folders from the bsconfig
/// 2. Take the (by then deduplicated) packages, and find all the '.re', '.res', '.ml' and
///    interface files.
/// The two step process is there to reduce IO overhead
pub fn make(filter: &Option<regex::Regex>, root_folder: &str) -> AHashMap<String, Package> {
    /* The build_package get's called recursively. By using extend, we deduplicate all the packages
     * */
    let mut map: AHashMap<String, Package> = AHashMap::new();

    let package_dir = get_package_dir("", true);
    let bsconfig = read_bsconfig(&package_dir);
    build_package(&mut map, bsconfig, &package_dir, root_folder, true);
    /* Once we have the deduplicated packages, we can add the source files for each - to minimize
     * the IO */
    let result = extend_with_children(&filter, map);
    result
        .values()
        .into_iter()
        .for_each(|package| match &package.dirs {
            Some(dirs) => dirs.iter().for_each(|dir| {
                let _ = std::fs::create_dir_all(
                    std::path::Path::new(&helpers::get_bs_build_path(root_folder, &package.name))
                        .join(dir),
                );
            }),
            None => (),
        });
    result
}

pub fn get_package_name(path: &str) -> String {
    let bsconfig = read_bsconfig(&path);
    bsconfig.name
}
