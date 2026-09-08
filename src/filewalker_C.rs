use crate::database::Database;
use crate::filewalker::FileEntry;
use crate::filewalker_C::FileIndexEvent::{InsertFile, NeedsHash, UpdateHash};
use crossbeam_channel::{Receiver, SendError, Sender};
use log::{debug, error, info};
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
    NeedsHash {
        path: PathBuf,
    },
}

pub(crate) fn walk_and_hash(db: Database, path: PathBuf) -> Result<(), std::io::Error> {
    let (sender, receiver) = crossbeam_channel::bounded(CHANNEL_CAP);

    let db_worker = {
        let sender = sender.clone();
        let receiver = receiver.clone();
        let db_worker = thread::spawn(move || {
            db_worker(db, receiver, sender)?;
            Ok::<(), SendError<FileIndexEvent>>(())
        });
        db_worker
    };

    {
        let sender = sender.clone();
        thread::spawn(move || {
            collect_files_worker(&path, sender);
        });
    }

    for i in 0..WORKER_COUNT {
        {
            let sender = sender.clone();
            let receiver = receiver.clone();
            thread::spawn(move || hash_worker(receiver, sender));
        }
    }

    db_worker.join().unwrap();
    Ok(())
}

fn db_worker(
    db: Database,
    input: Receiver<FileIndexEvent>,
    output: Sender<FileIndexEvent>,
) -> Result<(), SendError<FileIndexEvent>> {
    for msg in input {
        match msg {
            InsertFile { path, size, mtime } => {
                db.insert_file(&path, size, mtime)
                    .expect("Database could not be written!");

                if db
                    .has_valid_hash(&path, mtime)
                    .expect("Database could not be read!")
                {
                    debug!("Database worker requests hash for {}", path.display());
                    output.send(NeedsHash { path })?;
                }
            }
            UpdateHash { path, hash } => {
                db.update_hash(&path, &hash)
                    .expect("Database could not be written!");
            }
            _ => {}
        }
    }
    Ok(())
}

fn hash_worker(
    input: Receiver<FileIndexEvent>,
    output: Sender<FileIndexEvent>,
) -> Result<(), SendError<FileIndexEvent>> {
    for msg in input {
        match msg {
            FileIndexEvent::NeedsHash { path } => {
                let hash = generate_hash(&path);
                if let Ok(hash) = hash {
                    debug!("Hash worker publishes hash for {}", path.display());
                    output.send(FileIndexEvent::UpdateHash { path, hash })?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn collect_files_worker(path: &Path, output: Sender<FileIndexEvent>) {
    let path = path.to_path_buf();

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
                output.send(msg).unwrap();
            }
        }
    }
    drop(output);
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
