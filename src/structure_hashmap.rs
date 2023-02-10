use crate::helpers::LexicalAbsolute;
use ahash::AHashMap;
use std::path::PathBuf;
use std::{error, fs};

pub fn read_folders(
    path: &str,
    recurse: bool,
) -> Result<AHashMap<String, fs::Metadata>, Box<dyn error::Error>> {
    let mut map: AHashMap<String, fs::Metadata> = AHashMap::new();

    let path_buf = PathBuf::from(path);
    let abs_path = path_buf
        .to_lexical_absolute()
        .map(|x| x.to_str().map(|y| y.to_string()).unwrap_or("".to_string()))
        .and_then(|x| fs::metadata(x.to_owned()).map(|m| (x.to_owned(), m)));

    for entry in fs::read_dir(path.replace("//", "/"))? {
        let path_buf = entry.map(|entry| entry.path())?;
        let metadata = fs::metadata(&path_buf)?;
        let name = path_buf
            .file_name()
            .and_then(|x| x.to_str())
            .unwrap_or("Unknown")
            .to_string();

        let path_ext = path_buf.extension().and_then(|x| x.to_str());

        if metadata.file_type().is_dir() && recurse {
            match read_folders(&(path.to_owned() + "/" + &name + "/"), recurse) {
                Ok(s) => map.extend(s),
                Err(e) => println!("Error reading directory: {}", e),
            }
        }
        match path_ext {
            Some("res") | Some("ml") | Some("re") | Some("resi") | Some("rei") | Some("mli") => {
                match abs_path {
                    Ok((ref path, _)) => {
                        map.insert(path.to_owned() + "/" + &name, metadata);
                    }
                    Err(ref e) => println!("Error reading directory: {}", e),
                }
            }
            _ => (),
        }
    }

    Ok(map)
}
