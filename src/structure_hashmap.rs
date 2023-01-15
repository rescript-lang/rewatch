use ahash::AHashMap;
use std::{error, fs};

pub fn read_structure(
    path: &str,
    extension: &str,
    recurse: bool,
) -> Result<AHashMap<String, fs::Metadata>, Box<dyn error::Error>> {
    let mut map: AHashMap<String, fs::Metadata> = AHashMap::new();

    for entry in fs::read_dir(path.replace("//", "/"))? {
        let path_buf = entry.map(|entry| entry.path())?;
        let metadata = fs::metadata(&path_buf)?;
        let name = path_buf
            .file_name()
            .and_then(|x| x.to_str())
            .unwrap_or("Unknown")
            .to_string();

        if metadata.file_type().is_dir() && recurse {
            match read_structure(&(path.to_owned() + "/" + &name + "/"), extension, recurse) {
                Ok(s) => map.extend(s),
                Err(e) => println!("Error reading directory: \n {}", e),
            }
        } else if path_buf.extension().and_then(|x| x.to_str()) == Some(extension) {
            map.insert(name, metadata);
        }
    }

    Ok(map)
}
