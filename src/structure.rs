use std::{error, fmt, fs};

#[derive(Debug, Clone)]
pub enum T {
    File(String, fs::Metadata),
    Dir(String, Box<Vec<T>>),
}

impl fmt::Display for T {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", print_rec(self, 0))
    }
}

fn print_rec(elem: &T, depth: usize) -> String {
    match elem {
        T::File(name, _metadata) => "-".repeat(depth) + &name.to_string(),
        T::Dir(name, dir) => {
            let str = "-".repeat(depth) + &name.to_string() + "\n";

            let subdir = match &*dir {
                xs => xs
                    .to_owned()
                    .into_iter()
                    .map(|elem| print_rec(&elem, depth + 1))
                    .collect::<Vec<String>>()
                    .join("\n")
                    .to_string(),
            };

            str + &subdir
        }
    }
}

pub fn read_structure(path: &str) -> Result<T, Box<dyn error::Error>> {
    let mut structure = vec![];

    for entry in fs::read_dir(path)? {
        let path_buf = entry.map(|entry| entry.path())?;
        let metadata = fs::metadata(&path_buf)?;
        let name = path_buf
            .file_name()
            .and_then(|x| x.to_str())
            .unwrap_or("Unknown")
            .to_string();

        if metadata.file_type().is_dir() {
            match read_structure(&(path.to_owned() + "/" + &name + "/")) {
                Ok(s) => structure.push(s),
                Err(e) => println!("Error reading directory: \n {}", e),
            }
        } else {
            structure.push(T::File(name, metadata))
        }
    }

    Ok(T::Dir(path.to_string(), Box::new(structure)))
}
