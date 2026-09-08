use std::path::PathBuf;
use log::{error, info};
use crate::filewalker::FileEntry;

pub(crate) fn walk_and_hash(path: PathBuf) -> Result<(), std::io::Error> {
    use rayon::prelude::*;

    Ok(())
}