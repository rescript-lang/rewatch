use crate::package_tree;
use ahash::AHashSet;
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq)]
pub enum ParseState {
    Pending,
    ParseError,
    Warning,
    Success,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompileState {
    Pending,
    Error,
    Warning,
    Success,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Interface {
    pub path: String,
    pub parse_state: ParseState,
    pub compile_state: CompileState,
    pub last_modified: SystemTime,
    pub dirty: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Implementation {
    pub path: String,
    pub parse_state: ParseState,
    pub compile_state: CompileState,
    pub last_modified: SystemTime,
    pub dirty: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceFile {
    pub implementation: Implementation,
    pub interface: Option<Interface>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MlMap {
    pub dirty: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SourceType {
    SourceFile(SourceFile),
    MlMap(MlMap),
}

#[derive(Debug, Clone)]
pub struct Module {
    pub source_type: SourceType,
    pub deps: AHashSet<String>,
    pub package: package_tree::Package,
}
