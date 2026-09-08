use crate::filewalker::FileEntry;
use log::{error, info};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use walkdir::WalkDir;

pub(crate) fn walk_and_hash(path: PathBuf) -> Result<(), std::io::Error> {
    let (sender, receiver) = crossbeam_channel::bounded(5);

    let mut threads = Vec::new();

    for i in 0..5 {
        {
            let sender = sender.clone();
            threads.push(thread::spawn(move || {
                for j in 1..100 {
                    sender.send(i * 100 + j).unwrap();
                    thread::sleep(Duration::from_millis(100));
                }
                drop(sender);
            }));
        }
    }

    for i in 0..5 {
        {
            let receiver = receiver.clone();
            threads.push(thread::spawn(move || {
                for msg in receiver.iter() {
                    info!("Received: {}", msg);
                }
            }));
        }
    }

    drop(sender);
    for thread in threads {
        thread.join().unwrap();
    }

    Ok(())
}
