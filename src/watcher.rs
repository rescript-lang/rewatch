use crate::build;
use notify::{Config, RecursiveMode};
use notify_debouncer_mini::new_debouncer_opt;
use std::path::PathBuf;
use std::time::Duration;

pub static FILE_EXTENSIONS: &[&str] = &["re", "res", "ml", "mli", "rei", "resi"];

pub fn start(filter: &Option<regex::Regex>, folder: &str) {
    let (tx, rx) = std::sync::mpsc::channel();

    let mut debouncer = new_debouncer_opt::<_, notify::RecommendedWatcher>(
        Duration::from_millis(200),
        None,
        tx,
        Config::default(),
    )
    .unwrap();

    debouncer
        .watcher()
        .watch(folder.as_ref(), RecursiveMode::Recursive)
        .unwrap();

    for events in rx {
        match events {
            Ok(events) => {
                let paths = events
                    .iter()
                    .filter_map(|event| {
                        let path_buf = event.path.to_path_buf();
                        let name = path_buf
                            .file_name()
                            .and_then(|x| x.to_str())
                            .unwrap_or("Unknown")
                            .to_string();
                        let extension = path_buf.extension().and_then(|ext| ext.to_str());

                        match extension {
                            Some(extension)
                                if filter
                                    .as_ref()
                                    .map(|re| !re.is_match(&name))
                                    .unwrap_or(true)
                                    && FILE_EXTENSIONS.contains(&extension) =>
                            {
                                Some(path_buf)
                            }
                            _ => None,
                        }
                    })
                    .collect::<Vec<PathBuf>>();

                if paths.len() > 0 {
                    let _ = build::build(&filter, &folder);
                }
            }
            Err(_) => (),
        }
    }
}
