use crate::database::Database;
use crate::filewalker::FileEntry;
use crate::filewalker_C::FileIndexEvent::{InsertFile, UpdateHash};
use crossbeam_channel::{Receiver, SendError};
use log::{error, info};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{io, thread};
use walkdir::WalkDir;

const CHANNEL_CAP: usize = 10000;
const WORKER_COUNT: usize = 10;

#[derive(Debug, Clone)]
pub(crate) enum FileIndexEvent {
    InsertFile {
        path: PathBuf,
        size: u64,
        mtime: i64,
    },
    UpdateHash {
        path: PathBuf,
        hash: [u8; 32],
    },
}

pub(crate) fn walk_and_hash(db: Database, path: PathBuf) -> Result<(), std::io::Error> {
    let (db_sender, db_receiver) = crossbeam_channel::bounded(CHANNEL_CAP);

    let db_worker = thread::spawn(move || {
        for msg in db_receiver {
            match msg {
                InsertFile { path, size, mtime } => {
                    db.insert_file(Path::new(&path), size, mtime)
                        .expect("Database could not be written!");
                }
                UpdateHash { path, hash } => {
                    db.update_hash(&path, &hash)
                        .expect("Database could not be written!");
                }
            }
        }
    });

    let file_queue = collect_files_parallel(&path);

    for i in 0..WORKER_COUNT {
        {
            let file_queue = file_queue.clone();
            let db_sender = db_sender.clone();
            thread::spawn(move || {
                for msg in file_queue {
                    db_sender.send(msg.clone())?;
                    match process_event(msg) {
                        Ok(event) => {
                            if let Some(event) = event {
                                db_sender.send(event)?;
                            }
                        }
                        Err(e) => error!("{}", e),
                    }
                }
                Ok::<(), SendError<FileIndexEvent>>(())
            });
        }
    }

    db_worker.join().unwrap();
    Ok(())
}

fn process_event(event: FileIndexEvent) -> Result<Option<FileIndexEvent>, std::io::Error> {
    Ok(match event {
        FileIndexEvent::InsertFile { path, size, mtime } => {
            let hash = generate_hash(&path)?;
            Some(UpdateHash { path, hash })
        }
        FileIndexEvent::UpdateHash { .. } => None,
    })
}

pub(crate) fn collect_files_parallel(path: &Path) -> Receiver<FileIndexEvent> {
    let (sender, receiver) = crossbeam_channel::bounded(CHANNEL_CAP);
    let path = path.to_path_buf();
    thread::spawn(move || {
        for entry in WalkDir::new(path) {
            if let Ok(entry) = entry {
                if entry.file_type().is_file() {
                    // result.push(entry.into_path());
                    let (size, mtime) = if let Ok(meta) = entry.metadata() {
                        (meta.len(), meta.mtime())
                    } else {
                        (0, 0)
                    };

                    let msg = InsertFile {
                        path: entry.path().to_path_buf(),
                        size,
                        mtime,
                    };
                    sender.send(msg).unwrap();
                }
            }
        }
        drop(sender);
    });
    receiver
}

fn generate_hash(path: &Path) -> Result<[u8; 32], io::Error> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);

    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024]; // 1 MB

    loop {
        let bytes_read = reader.read(&mut buffer)?;

        if bytes_read == 0 {
            break;
        }

        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hasher.finalize().into())
}
