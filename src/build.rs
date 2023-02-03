use crate::package_tree;
use ahash::AHashMap;

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub dirty: bool,
    pub package: String,
}

pub fn get_source_files(
    packages: AHashMap<String, package_tree::Package>,
) -> AHashMap<String, SourceFile> {
    let mut files: AHashMap<String, SourceFile> = AHashMap::new();

    packages
        .iter()
        .for_each(|(package_name, package)| match &package.source_files {
            None => (),
            Some(source_files) => source_files.iter().for_each(|(file, _)| {
                files.insert(
                    file.to_owned(),
                    SourceFile {
                        dirty: true,
                        package: package_name.to_owned(),
                    },
                );
            }),
        });

    files
}
