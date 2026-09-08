use crate::filewalker_parallel::DbMessage::InsertFile;
use std::os::unix::prelude::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::{io, thread};
use walkdir::WalkDir;
use crate::filewalker::FileEntry;

#[derive(Debug, Clone)]
pub(crate) enum DbMessage {
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

pub(crate) fn collect_files_parallel(path: &Path) -> Receiver<DbMessage> {
    let (sender, receiver) = channel();
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
    });
    receiver
}

fn visit_files(receiver: Receiver<DbMessage>) -> Receiver<DbMessage> {
    let (sender, result_receiver) = channel();


    result_receiver
}

// #[cfg(test)]
// mod tests {
//     use crate::filewalker_parallel::{DbMessage, collect_files_parallel};
//     use std::path::Path;
//
//     #[test]
//     fn collects_files() {
//         let receiver = collect_files_parallel(Path::new("/tmp/"));
//         let messages: Vec<DbMessage> = receiver.iter().collect();
//         println!("{:?}", messages);
//     }
// }
