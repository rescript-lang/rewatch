use crate::build;
use crate::helpers;
use futures::{
    channel::mpsc::{channel, Receiver},
    SinkExt, StreamExt,
};
use futures_timer::Delay;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::time::Duration;

// type Queue = Vec<notify::Event>;

async fn async_watch(path: &str, filter: &Option<regex::Regex>) -> notify::Result<()> {
    let queue = std::sync::Mutex::new(Vec::new());
    // Automatically select the best implementation for your platform.
    // You can also access each implementation directly e.g. INotifyWatcher.
    let mut watcher = RecommendedWatcher::new(
        |res: Result<notify::Event, notify::Error>| match res {
            Ok(event) => {
                let _ = queue.lock().unwrap().push(event.to_owned());
            }
            Err(e) => println!("watch error: {:?}", e),
        },
        Config::default(),
    )
    .unwrap();

    watcher.watch(path.as_ref(), RecursiveMode::Recursive)?;

    loop {
        let events = &queue.lock().unwrap().to_vec();
        let needs_compile = events.iter().any(|event| {
            event.paths.iter().any(|path| {
                let path_buf = path.to_path_buf();
                let name = path_buf
                    .file_name()
                    .and_then(|x| x.to_str())
                    .unwrap_or("Unknown")
                    .to_string();

                let extension = path_buf.extension().and_then(|ext| ext.to_str());
                match extension {
                    Some(extension) => {
                        (helpers::is_implementation_file(&extension)
                            || helpers::is_interface_file(&extension))
                            && filter
                                .as_ref()
                                .map(|re| !re.is_match(&name))
                                .unwrap_or(true)
                    }

                    _ => false,
                }
            })
        });

        if needs_compile {
            // we wait for a bit before starting the compile as a debouncer
            let delay = Duration::from_millis(200);
            Delay::new(delay).await;

            // we drain the channel to avoid triggering multiple compiles

            let _ = build::build(filter, path);
        }
    }
}

pub fn start(filter: &Option<regex::Regex>, folder: &str) {
    futures::executor::block_on(async {
        if let Err(e) = async_watch(folder, filter).await {
            println!("error: {:?}", e)
        }
    });
}
