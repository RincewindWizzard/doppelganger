use crate::database::Database;
use crate::file_walker::DBCall::{InsertFile, UpdateHash};

use crate::file_walker::DBResultMessage::NeedsHash;
use crate::file_walker::FileWalkerMessage::FileFound;
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

const CHANNEL_CAP: usize = 10000;
const WORKER_COUNT: usize = 10;

enum FileWalkerMessage {
    FileFound {
        path: PathBuf,
        mtime: i64,
        size: u64,
    },
}

enum DBCall {
    InsertFile {
        path: PathBuf,
        mtime: i64,
        size: u64,
    },
    UpdateHash {
        path: PathBuf,
        mtime: i64,
        hash: [u8; 32],
    },
}

enum DBResultMessage {
    NeedsHash { path: PathBuf },
}

pub(crate) fn walk_and_hash(db: Database, path: PathBuf) -> Result<(), std::io::Error> {
    let (db_sender, db_receiver) = crossbeam_channel::unbounded();
    let (hash_sender, hash_receiver) = crossbeam_channel::unbounded();
    {
        let db_sender = db_sender.clone();
        thread::spawn(move || {
            collect_files_worker(&path, db_sender);
        });
    }

    let db_worker = {
        let sender = hash_sender.clone();
        let receiver = db_receiver.clone();
        let db_worker = thread::spawn(move || db_worker(db, receiver, sender));
        db_worker
    };

    for i in 0..WORKER_COUNT {
        {
            let sender = db_sender.clone();
            let receiver = hash_receiver.clone();
            thread::spawn(move || hash_worker(receiver, sender));
        }
    }

    db_worker.join().unwrap().unwrap();
    Ok(())
}

fn db_worker(
    db: Database,
    input: Receiver<DBCall>,
    output: Sender<DBResultMessage>,
) -> Result<(), SendError<DBResultMessage>> {
    for msg in input {
        match msg {
            InsertFile { path, size, mtime } => {
                db.insert_file(&path, size, mtime)
                    .expect("Database could not be written!");

                if !db
                    .has_valid_hash(&path, mtime)
                    .expect("Database could not be read!")
                {
                    debug!("Database worker requests hash for {}", path.display());
                    output.send(NeedsHash { path })?;
                }
            }
            UpdateHash { path, mtime, hash } => {
                db.update_hash(&path, &hash, mtime)
                    .expect("Database could not be written!");
                info!("Hash updated: {}: {}", path.display(), hex::encode(hash));
            }
            _ => {}
        }
    }
    Ok(())
}

fn hash_worker(
    input: Receiver<DBResultMessage>,
    output: Sender<DBCall>,
) -> Result<(), SendError<DBCall>> {
    for msg in input {
        match msg {
            NeedsHash { path } => {
                if let Ok((mtime, hash)) = generate_hash(&path) {
                    debug!("Hash worker publishes hash for {}", path.display());

                    output.send(UpdateHash { path, mtime, hash })?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn collect_files_worker(path: &Path, output: Sender<DBCall>) {
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
