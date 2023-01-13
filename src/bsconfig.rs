use serde::Deserialize;
use std::fs;

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum Subdirs {
    Qualified(Vec<Source>),
    All(bool),
}
#[derive(Deserialize, Debug, Clone)]
pub struct QualifiedSource {
    pub dir: String,
    pub subdirs: Option<Subdirs>,
    #[serde(rename = "type")]
    pub type_: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum Source {
    ShortHand(Vec<String>),
    Qualified(Vec<QualifiedSource>),
    SingleShortHand(String),
    Single(QualifiedSource),
}

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum Sources {
    Multiple(Vec<Source>),
    Single(Source),
}

#[derive(Deserialize, Debug, Clone)]
pub struct PackageSpec {
    pub module: String,
    #[serde(rename = "in-source")]
    pub in_source: bool,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum PackageSpecs {
    Multiple(Vec<PackageSpec>),
    Single(PackageSpec),
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
pub enum PPXFlags {
    Multiple(Vec<String>),
    Single(String),
}

#[derive(Deserialize, Debug, Clone)]
pub struct T {
    pub name: String,
    pub sources: Sources,
    #[serde(rename = "package-specs")]
    pub package_specs: Option<PackageSpecs>,
    pub warnings: Option<Warnings>,
    pub suffix: Option<String>,
    #[serde(rename = "pinned-dependencies")]
    pub pinned_dependencies: Option<Vec<String>>,
    #[serde(rename = "bs-dependencies")]
    pub bs_dependencies: Option<Vec<String>>,
    #[serde(rename = "ppx-flags")]
    pub ppx_flags: Option<Vec<PPXFlags>>,
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
