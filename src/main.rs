pub mod structure;
pub mod watcher;

fn main() {
    let structure = structure::read_structure("dir");

    match structure {
        Ok(s) => println!("{}", s),
        Err(_) => println!("Could not read fs"),
    }

    futures::executor::block_on(async {
        if let Err(e) = watcher::async_watch("dir").await {
            println!("error: {:?}", e)
        }
    });
}
