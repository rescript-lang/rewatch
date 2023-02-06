use rayon::prelude::*;
use serde::Deserialize;
use std::fs;

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum OneOrMore<T> {
    Multiple(Vec<T>),
    Single(T),
}

#[derive(Deserialize, Debug, Clone, PartialEq, Hash)]
#[serde(untagged)]
pub enum Subdirs {
    Qualified(Vec<Source>),
    Recurse(bool),
}
impl Eq for Subdirs {}

#[derive(Deserialize, Debug, Clone, PartialEq, Hash)]
pub struct PackageSource {
    pub dir: String,
    pub subdirs: Option<Subdirs>,
    #[serde(rename = "type")]
    pub type_: Option<String>,
}

/// `to_qualified_without_children` takes a tree like structure of dependencies, coming in from
/// `bsconfig`, and turns it into a flat list. The main thing we extract here are the source
/// folders, and optional subdirs, where potentially, the subdirs recurse or not.
pub fn to_qualified_without_children(s: &Source) -> PackageSource {
    match s {
        Source::Shorthand(dir) => PackageSource {
            dir: dir.to_owned(),
            subdirs: None,
            type_: None,
        },
        Source::Qualified(PackageSource {
            dir,
            type_,
            subdirs: Some(Subdirs::Recurse(should_recurse)),
        }) => PackageSource {
            dir: dir.to_owned(),
            subdirs: Some(Subdirs::Recurse(*should_recurse)),
            type_: type_.to_owned(),
        },
        Source::Qualified(PackageSource { dir, type_, .. }) => PackageSource {
            dir: dir.to_owned(),
            subdirs: None,
            type_: type_.to_owned(),
        },
    }
}

impl Eq for PackageSource {}

#[derive(Deserialize, Debug, Clone, PartialEq, Hash)]
#[serde(untagged)]
pub enum Source {
    Shorthand(String),
    Qualified(PackageSource),
}
impl Eq for Source {}

#[derive(Deserialize, Debug, Clone)]
pub struct PackageSpec {
    pub module: String,
    #[serde(rename = "in-source")]
    pub in_source: bool,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum Error {
    Catchall(bool),
    Qualified(String),
}

#[derive(Deserialize, Debug, Clone)]
pub struct Warnings {
    pub number: Option<String>,
    pub error: Option<Error>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Reason {
    #[serde(rename = "react-jsx")]
    pub react_jsx: i32,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum Namespace {
    Bool(bool),
    String(String),
}

/// # bsconfig.json representation
/// This is tricky, there is a lot of ambiguity. This is probably incomplete.
#[derive(Deserialize, Debug, Clone)]
pub struct T {
    pub name: String,
    pub sources: OneOrMore<Source>,
    #[serde(rename = "package-specs")]
    pub package_specs: Option<OneOrMore<PackageSpec>>,
    pub warnings: Option<Warnings>,
    pub suffix: Option<String>,
    #[serde(rename = "pinned-dependencies")]
    pub pinned_dependencies: Option<Vec<String>>,
    #[serde(rename = "bs-dependencies")]
    pub bs_dependencies: Option<Vec<String>>,
    #[serde(rename = "ppx-flags")]
    pub ppx_flags: Option<Vec<OneOrMore<String>>>,
    #[serde(rename = "bsc-flags")]
    pub bsc_flags: Option<Vec<OneOrMore<String>>>,
    pub reason: Option<Reason>,
    pub namespace: Option<Namespace>,
}

/// This flattens string flags
pub fn flatten_flags(flags: &Option<Vec<OneOrMore<String>>>) -> Vec<String> {
    match flags {
        None => vec![],
        Some(xs) => xs
            .iter()
            .map(|x| match x {
                OneOrMore::Single(y) => vec![y.to_owned()],
                OneOrMore::Multiple(ys) => ys.to_owned(),
            })
            .flatten()
            .collect::<Vec<String>>()
            .iter()
            .map(|str| str.split(" "))
            .flatten()
            .map(|str| str.to_string())
            .collect::<Vec<String>>(),
    }
}

/// Since ppx-flags could be one or more, and could be nested potentiall, this function takes the
/// flags and flattens them outright.
pub fn flatten_ppx_flags(
    node_modules_dir: &String,
    flags: &Option<Vec<OneOrMore<String>>>,
) -> Vec<String> {
    match flags {
        None => vec![],
        Some(xs) => xs
            .par_iter()
            .map(|x| match x {
                OneOrMore::Single(y) => {
                    vec!["-ppx".to_string(), node_modules_dir.to_owned() + "/" + y]
                }
                OneOrMore::Multiple(ys) if ys.len() == 0 => vec![],
                OneOrMore::Multiple(ys) => vec![
                    "-ppx".to_string(),
                    vec![node_modules_dir.to_owned() + "/" + &ys[0]]
                        .into_iter()
                        .chain(ys[1..].to_owned())
                        .collect::<Vec<String>>()
                        .join(" "),
                ],
            })
            .flatten()
            .collect::<Vec<String>>(),
    }
}

/// Try to convert a bsconfig from a certain path to a bsconfig struct
pub fn read(path: String) -> T {
    fs::read_to_string(path.clone())
        .map_err(|e| format!("Could not read bsconfig. {path} - {e}"))
        .and_then(|x| {
            serde_json::from_str::<T>(&x)
                .map_err(|e| format!("Could not parse bsconfig. {path} - {e}"))
        })
        .expect("Errors reading bsconfig")
}
