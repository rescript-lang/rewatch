use crate::build;
use crate::build::build_types::SourceType;
use crate::cmd;
use crate::helpers;
use crate::helpers::emojis::*;
use crate::queue::FifoQueue;
use crate::queue::*;
use futures_timer::Delay;
use notify::event::ModifyKind;
use notify::{Config, Error, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
enum CompileType {
    Incremental,
    Full,
    None,
}

fn is_rescript_file(path_buf: &PathBuf) -> bool {
    let extension = path_buf.extension().and_then(|ext| ext.to_str());

    if let Some(extension) = extension {
        helpers::is_implementation_file(&extension) || helpers::is_interface_file(&extension)
    } else {
        false
    }
}

fn is_in_build_path(path_buf: &PathBuf) -> bool {
    path_buf
        .to_str()
        .map(|x| x.contains("/lib/bs/") || x.contains("/lib/ocaml/"))
        .unwrap_or(false)
}

fn matches_filter(path_buf: &PathBuf, filter: &Option<regex::Regex>) -> bool {
    let name = path_buf
        .file_name()
        .map(|x| x.to_string_lossy().to_string())
        .unwrap_or("".to_string());
    filter.as_ref().map(|re| !re.is_match(&name)).unwrap_or(true)
}

async fn async_watch(
    q: Arc<FifoQueue<Result<Event, Error>>>,
    path: &str,
    filter: &Option<regex::Regex>,
    after_build: Option<String>,
) -> notify::Result<()> {
    let mut build_state = build::initialize_build(None, filter, path).expect("Can't initialize build");
    let mut needs_compile_type = CompileType::Incremental;
    let mut initial_build = true;

    loop {
        let mut events: Vec<Event> = vec![];
        if !q.is_empty() {
            // Wait for events to settle
            Delay::new(Duration::from_millis(50)).await;
        }
        while !q.is_empty() {
            match q.pop() {
                Ok(event) => events.push(event),
                Err(_) => (),
            }
        }

        for event in events {
            let paths = event
                .paths
                .iter()
                .filter(|path| is_rescript_file(path))
                .filter(|path| !is_in_build_path(path))
                .filter(|path| matches_filter(path, filter));
            for path in paths {
                let path_buf = path.to_path_buf();

                match (needs_compile_type, event.kind) {
                    (
                        CompileType::Incremental | CompileType::None,
                        // when we have a name change, create or remove event we need to do a full compile
                        EventKind::Remove(_)
                        | EventKind::Any
                        | EventKind::Create(_)
                        | EventKind::Modify(ModifyKind::Name(_)),
                    ) => {
                        // if we are going to do a full compile, we don't need to bother marking
                        // files dirty because we do a full scan anyway
                        needs_compile_type = CompileType::Full;
                    }

                    (
                        CompileType::None | CompileType::Incremental,
                        // when we have a data change event, we can do an incremental compile
                        EventKind::Modify(ModifyKind::Data(_)),
                    ) => {
                        // if we are going to compile incrementally, we need to mark the exact files
                        // dirty
                        if let Ok(canonicalized_path_buf) = path_buf.canonicalize() {
                            for module in build_state.modules.values_mut() {
                                match module.source_type {
                                    SourceType::SourceFile(ref mut source_file) => {
                                        // mark the implementation file dirty
                                        let package = build_state
                                            .packages
                                            .get(&module.package_name)
                                            .expect("Package not found");
                                        let canonicalized_implementation_file =
                                            std::path::PathBuf::from(package.path.to_string())
                                                .join(source_file.implementation.path.to_string());
                                        if canonicalized_path_buf == canonicalized_implementation_file {
                                            if let Ok(modified) =
                                                canonicalized_path_buf.metadata().and_then(|x| x.modified())
                                            {
                                                source_file.implementation.last_modified = modified;
                                            };
                                            source_file.implementation.parse_dirty = true;
                                            break;
                                        }

                                        // mark the interface file dirty
                                        if let Some(ref mut interface) = source_file.interface {
                                            let canonicalized_interface_file =
                                                std::path::PathBuf::from(package.path.to_string())
                                                    .join(interface.path.to_string());
                                            if canonicalized_path_buf == canonicalized_interface_file {
                                                if let Ok(modified) = canonicalized_path_buf
                                                    .metadata()
                                                    .and_then(|x| x.modified())
                                                {
                                                    interface.last_modified = modified;
                                                }
                                                interface.parse_dirty = true;
                                                break;
                                            }
                                        }
                                    }
                                    SourceType::MlMap(_) => (),
                                }
                            }
                            needs_compile_type = CompileType::Incremental;
                        }
                    }

                    (
                        CompileType::None | CompileType::Incremental,
                        // these are not relevant events for compilation
                        EventKind::Access(_)
                        | EventKind::Other
                        | EventKind::Modify(ModifyKind::Any)
                        | EventKind::Modify(ModifyKind::Metadata(_))
                        | EventKind::Modify(ModifyKind::Other),
                    ) => (),
                    // if we already need a full compile, we don't need to check for other events
                    (CompileType::Full, _) => (),
                }
            }
        }
        match needs_compile_type {
            CompileType::Incremental => {
                let timing_total = Instant::now();
                match build::incremental_build(
                    &mut build_state,
                    None,
                    initial_build,
                    if initial_build { false } else { true },
                ) {
                    Ok(_) => {
                        after_build.clone().map(|command| cmd::run(command));
                        let timing_total_elapsed = timing_total.elapsed();
                        println!(
                            "\n{}{}Finished {} compilation in {:.2}s\n",
                            LINE_CLEAR,
                            SPARKLES,
                            if initial_build { "initial" } else { "incremental" },
                            timing_total_elapsed.as_secs_f64()
                        );
                    }
                    Err(_) => (),
                }
                needs_compile_type = CompileType::None;
                initial_build = false;
            }
            CompileType::Full => {
                let timing_total = Instant::now();
                build_state = build::initialize_build(None, filter, path).expect("Can't initialize build");
                let _ = build::incremental_build(&mut build_state, None, initial_build);
                after_build.clone().map(|command| cmd::run(command));
                let timing_total_elapsed = timing_total.elapsed();
                println!(
                    "{}\r{}Finished compilation in {:.2}s",
                    LINE_CLEAR,
                    CHECKMARK,
                    timing_total_elapsed.as_secs_f64()
                );
                needs_compile_type = CompileType::None;
                initial_build = false;
            }
            CompileType::None => {
                // We want to sleep for a little while so the CPU can schedule other work. That way we end
                // up not burning CPU cycles.
                Delay::new(Duration::from_millis(50)).await;
            }
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
