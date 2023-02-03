use std::fs;
use std::path::PathBuf;

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
