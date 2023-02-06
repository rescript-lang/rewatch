use std::fs;
use std::path::PathBuf;

pub fn get_package_path(root: &str, package_name: &str) -> String {
    format!("{}/node_modules/{}", root, package_name)
}

pub fn get_build_path(root: &str, package_name: &str) -> String {
    format!("{}/node_modules/{}/_build", root, package_name)
}

pub fn get_path(root: &str, package_name: &str, file: &str) -> String {
    format!("{}/{}/{}", root, package_name, file)
}

pub fn get_node_modules_path(root: &str) -> String {
    format!("{}/node_modules", root)
}

pub fn get_abs_path(path: &str) -> String {
    let abs_path_buf = PathBuf::from(path);
    return fs::canonicalize(abs_path_buf)
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

/// Capitalizes the first character in s.
fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

pub fn file_path_to_module_name(path: &str) -> String {
    capitalize(&get_basename(path))
}
