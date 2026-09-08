use crate::database::Database;

use crate::file_walker::FileWalkerEvent::FileFound;
use crate::file_walker::HashRequestMessage::NeedsHash;
use crate::file_walker::HashResultMessage::HashCalculated;
use crossbeam_channel::{Receiver, SendError, Sender};
use hex_literal::hex;
use log::{debug, error, info};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{io, thread};
use walkdir::WalkDir;

const CHANNEL_CAP: usize = 0;
const WORKER_COUNT: usize = 10;

enum FileWalkerEvent {
    FileFound {
        path: PathBuf,
        mtime: i64,
        size: u64,
    },
}

enum HashResultMessage {
    HashCalculated {
        path: PathBuf,
        mtime: i64,
        hash: [u8; 32],
    },
}

enum HashRequestMessage {
    NeedsHash { path: PathBuf },
}

pub(crate) fn walk_and_hash(db: Database, path: PathBuf) -> Result<(), std::io::Error> {
    let (file_walker_tx, file_walker_rx) = crossbeam_channel::bounded(CHANNEL_CAP);
    let (hash_request_tx, hash_request_rx) = crossbeam_channel::bounded(CHANNEL_CAP);
    let (hash_response_tx, hash_response_rx) = crossbeam_channel::bounded(CHANNEL_CAP);

    thread::spawn(move || {
        collect_files_worker(&path, file_walker_tx);
    });

    let db_worker = {
        let db_worker = thread::spawn(move || db_worker(db, file_walker_rx, hash_response_rx, hash_request_tx));
        db_worker
    };

    for i in 0..WORKER_COUNT {
        {
            let sender = hash_response_tx.clone();
            let receiver = hash_request_rx.clone();
            thread::spawn(move || hash_worker(receiver, sender));
        }
    }
    drop(hash_response_tx);
    drop(hash_request_rx);

    db_worker.join().unwrap().unwrap();
    Ok(())
}

fn db_worker(
    db: Database,
    file_rx: Receiver<FileWalkerEvent>,
    hash_results_rx: Receiver<HashResultMessage>,
    hash_request_tx: Sender<HashRequestMessage>,
) -> Result<(), SendError<HashRequestMessage>> {
    loop {
        // Hash Events have higher priority as they take longer
        if let Ok(msg) = hash_results_rx.try_recv() {
            match msg {
                HashCalculated { path, mtime, hash } => {
                    db.update_hash(&path, &hash, mtime)
                        .expect("Database could not be written!");
                    info!("Hash updated: {}: {}", path.display(), hex::encode(hash));
                }
            }
            continue;
        }

        // only feed new files if there are no hash events
        if let Ok(msg) = file_rx.try_recv() {
            match msg {
                FileFound { path, mtime, size } => {
                    db.insert_file(&path, size, mtime)
                        .expect("Database could not be written!");

                    if !db
                        .has_valid_hash(&path, mtime)
                        .expect("Database could not be read!")
                    {
                        debug!("Database worker requests hash for {}", path.display());
                        hash_request_tx.send(NeedsHash { path })?;
                    }
                }
            }
            continue;
        }
    }

    Ok(())
}

fn hash_worker(
    input: Receiver<HashRequestMessage>,
    output: Sender<HashResultMessage>,
) -> Result<(), SendError<HashResultMessage>> {
    for msg in input {
        match msg {
            NeedsHash { path } => {
                if let Ok((mtime, hash)) = generate_hash(&path) {
                    debug!("Hash worker publishes hash for {}", path.display());

                    output.send(HashCalculated { path, mtime, hash })?;
                }
            }
        }
    }
    Ok(())
}

fn collect_files_worker(path: &Path, output: Sender<FileWalkerEvent>) {
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

                let msg = FileFound {
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

fn generate_hash(path: &Path) -> Result<(i64, [u8; 32]), io::Error> {
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

    let mtime = path.metadata()?.mtime();

    Ok((mtime, hasher.finalize().into()))
}
