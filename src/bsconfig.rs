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
pub struct QualifiedSource {
    pub dir: String,
    pub subdirs: Option<Subdirs>,
    #[serde(rename = "type")]
    pub type_: Option<String>,
}

/* This needs a better name / place. Basically, we need to go from this tree like structure, to a flat list of dependencies. We don't want to keep the children's stuff around at this point. But we do want to keep the info regarding wether the directories fully recurse or not around...
Reason for going this route rather than any other is that we will have all the folders already, and we want them deduplicated so we only go through the sources once...
 * */
pub fn to_qualified_without_children(s: &Source) -> QualifiedSource {
    match s {
        Source::Shorthand(dir) => QualifiedSource {
            dir: dir.to_owned(),
            subdirs: None,
            type_: None,
        },
        Source::Qualified(QualifiedSource {
            dir,
            type_,
            subdirs: Some(Subdirs::Recurse(should_recurse)),
        }) => QualifiedSource {
            dir: dir.to_owned(),
            subdirs: Some(Subdirs::Recurse(*should_recurse)),
            type_: type_.to_owned(),
        },
        Source::Qualified(QualifiedSource { dir, type_, .. }) => QualifiedSource {
            dir: dir.to_owned(),
            subdirs: None,
            type_: type_.to_owned(),
        },
    }
}

impl Eq for QualifiedSource {}

#[derive(Deserialize, Debug, Clone, PartialEq, Hash)]
#[serde(untagged)]
pub enum Source {
    Shorthand(String),
    Qualified(QualifiedSource),
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

/// # bsconfig.json representation
///
/// Probably incomplete
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
    pub reason: Option<Reason>,
}

pub fn read(path: String) -> T {
    fs::read_to_string(path.clone())
        .map_err(|e| format!("Could not read bsconfig. {path} - {e}"))
        .and_then(|x| {
            serde_json::from_str::<T>(&x)
                .map_err(|e| format!("Could not parse bsconfig. {path} - {e}"))
        })
        .expect("Errors reading bsconfig")
}
