use std::fs;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

pub trait LexicalAbsolute {
    fn to_lexical_absolute(&self) -> std::io::Result<PathBuf>;
}

impl LexicalAbsolute for Path {
    fn to_lexical_absolute(&self) -> std::io::Result<PathBuf> {
        let mut absolute = if self.is_absolute() {
            PathBuf::new()
        } else {
            std::env::current_dir()?
        };
        for component in self.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    absolute.pop();
                }
                component @ _ => absolute.push(component.as_os_str()),
            }
        }
        Ok(absolute)
    }
}

pub fn get_package_path(root: &str, package_name: &str) -> String {
    format!("{}/node_modules/{}", root, package_name)
}

pub fn get_build_path(root: &str, package_name: &str) -> String {
    format!("{}/node_modules/{}/lib/ocaml", root, package_name)
}

pub fn get_bs_build_path(root: &str, package_name: &str) -> String {
    format!("{}/node_modules/{}/lib/bs", root, package_name)
}

pub fn get_path(root: &str, package_name: &str, file: &str) -> String {
    format!("{}/{}/{}", root, package_name, file)
}

pub fn get_node_modules_path(root: &str) -> String {
    format!("{}/node_modules", root)
}

pub fn get_abs_path(path: &str) -> String {
    let abs_path_buf = PathBuf::from(path);

    return abs_path_buf
        .to_lexical_absolute()
        .expect("Could not canonicalize")
        .to_str()
        .expect("Could not canonicalize")
        .to_string();
}

pub fn get_basename(path: &str) -> String {
    let path_buf = PathBuf::from(path);
    return path_buf
        .file_stem()
        .expect("Could not get basename")
        .to_str()
        .expect("Could not get basename")
        .to_string();
}

pub fn change_extension(path: &str, new_extension: &str) -> String {
    let path_buf = PathBuf::from(path);
    return path_buf
        .with_extension(new_extension)
        .to_str()
        .expect("Could not change extension")
        .to_string();
}

/// Capitalizes the first character in s.
fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

// this doesn't capitalize the module name! if the rescript name of the file is "foo.res" the
// compiler assets are foo-Namespace.cmt and foo-Namespace.cmj, but the module name is Foo
pub fn file_path_to_compiler_asset_basename(path: &str, namespace: &Option<String>) -> String {
    let base = get_basename(path);
    match namespace {
        Some(namespace) => base.to_string() + "-" + &namespace,
        None => base,
    }
}

pub fn file_path_to_module_name(path: &str, namespace: &Option<String>) -> String {
    capitalize(&file_path_to_compiler_asset_basename(path, namespace))
}

pub fn contains_ascii_characters(str: &str) -> bool {
    for chr in str.chars() {
        if chr.is_ascii_alphanumeric() {
            return true;
        }
    }
    return false;
}

pub fn create_build_path(build_path: &str) {
    fs::DirBuilder::new()
        .recursive(true)
        .create(PathBuf::from(build_path.to_string()))
        .unwrap();
}

pub fn get_bsc(root_path: &str) -> String {
    get_node_modules_path(root_path) + "/rescript/darwinarm64/bsc.exe"
}
