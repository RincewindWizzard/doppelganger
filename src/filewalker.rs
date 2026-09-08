use clap::Parser;
use log::{error, info};
use sha2::{Digest, Sha256};
use std::fmt::{Debug, Formatter};
use std::fs::File;
use std::io::{BufReader, Read};
use std::os::unix::prelude::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{io, thread};
use walkdir::WalkDir;

struct FilePath {
    path: Box<Path>,
    size: u64,
}

fn collect_files(path: &Path) -> Vec<PathBuf> {
    let mut result: Vec<PathBuf> = Vec::new();

    for entry in WalkDir::new(path) {
        if let Ok(entry) = entry {
            if entry.file_type().is_file() {
                result.push(entry.into_path());
            }
        }
    }
    result
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

#[derive(Debug)]
struct FileEntry {
    path: PathBuf,
    size: u64,
    hash: [u8; 32],
}
impl FileEntry {
    fn new(path: &Path, size: u64, hash: [u8; 32]) -> Self {
        Self {
            path: path.to_path_buf(),
            size,
            hash,
        }
    }
    fn hash_str(&self) -> String {
        hex::encode(self.hash)
    }
}

fn visit_file(path: &Path) -> Result<FileEntry, io::Error> {
    Ok(FileEntry::new(path, path.metadata()?.len(), generate_hash(path)?))
}
pub(crate) fn walk_and_hash(path: PathBuf) -> Result<(), std::io::Error> {
    use rayon::prelude::*;
    let paths: Vec<PathBuf> = collect_files(&path);

    let result: Vec<FileEntry> = paths
        .par_iter()
        .filter_map(|path| match visit_file(path.as_path()) {
            Ok(entry) => Some(entry),
            Err(err) => {
                error!("Failed to process {:?}: {}", path, err);
                None
            }
        })
        .map(|entry| {
            info!("Processed {:?}", entry);
            entry
        })
        .collect();
    println!("result: {:?}", result);
    Ok(())
}
