use crate::build;
use crate::cmd;
use crate::helpers;
use crate::queue::FifoQueue;
use crate::queue::*;
use futures_timer::Delay;
use notify::{Config, Error, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::Arc;
use std::time::Duration;

async fn async_watch(
    q: Arc<FifoQueue<Result<Event, Error>>>,
    path: &str,
    filter: &Option<regex::Regex>,
    after_build: Option<String>,
) -> notify::Result<()> {
    loop {
        let mut events: Vec<Event> = vec![];
        while !q.is_empty() {
            match q.pop() {
                Ok(event) => events.push(event),
                Err(_) => (),
            }
        }

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
            // Wait for events to settle
            Delay::new(Duration::from_millis(300)).await;

            // Flush any remaining events that came in before
            while !q.is_empty() {
                let _ = q.pop();
            }

            let _ = build::build(filter, path);
            after_build.clone().map(|command| cmd::run(command));
        }
    }
}

pub fn start(filter: &Option<regex::Regex>, folder: &str, after_build: Option<String>) {
    futures::executor::block_on(async {
        let queue = Arc::new(FifoQueue::<Result<Event, Error>>::new());
        let producer = queue.clone();
        let consumer = queue.clone();

        let mut watcher = RecommendedWatcher::new(move |res| producer.push(res), Config::default())
            .expect("Could not create watcher");
        watcher
            .watch(folder.as_ref(), RecursiveMode::Recursive)
            .expect("Could not start watcher");

        if let Err(e) = async_watch(consumer, folder, filter, after_build).await {
            println!("error: {:?}", e)
        }
    })
}
