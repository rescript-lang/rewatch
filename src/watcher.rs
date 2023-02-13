use crate::build;
use crate::helpers;
use futures::{
    channel::mpsc::{channel, Receiver},
    SinkExt, StreamExt,
};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
pub static FILE_TYPES: &[&str] = &["re", "res", "ml", "rei", "resi", "mli"];

fn async_watcher() -> notify::Result<(RecommendedWatcher, Receiver<notify::Result<Event>>)> {
    let (mut tx, rx) = channel(1);

    // Automatically select the best implementation for your platform.
    // You can also access each implementation directly e.g. INotifyWatcher.
    let watcher = RecommendedWatcher::new(
        move |res| {
            futures::executor::block_on(async {
                tx.send(res).await.unwrap();
            })
        },
        Config::default(),
    )?;

    Ok((watcher, rx))
}

async fn async_watch(path: String) -> notify::Result<()> {
    let (mut watcher, mut rx) = async_watcher()?;

    // Add a path to be watched. All files and directories at that path and
    // below will be monitored for changes.
    watcher.watch(path.as_ref(), RecursiveMode::Recursive)?;

    while let Some(res) = rx.next().await {
        let files_to_compile = res
            .iter()
            .map(|event| {
                event
                    .paths
                    .iter()
                    .map(|path| path.to_path_buf())
                    .filter(|path| helpers::string_ends_with_any(path, FILE_TYPES))
                    .collect::<Vec<PathBuf>>()
            })
            .flatten()
            .into_iter()
            .collect::<Vec<PathBuf>>();

        let delay = Duration::from_millis(10);
        if files_to_compile.len() > 0 {
            thread::sleep(delay);
            build::build(&path);
        }
    }

    Ok(())
}

pub fn start(folder: &str) {
    futures::executor::block_on(async {
        if let Err(e) = async_watch(folder.to_string()).await {
            println!("error: {:?}", e)
        }
    });
}
