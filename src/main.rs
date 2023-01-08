pub mod watcher;

fn main() {
    futures::executor::block_on(async {
        if let Err(e) = watcher::async_watch("./dir").await {
            println!("error: {:?}", e)
        }
    });
}

