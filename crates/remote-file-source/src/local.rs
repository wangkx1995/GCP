use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use walkdir::WalkDir;

pub(crate) fn collect_inputs(input: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
    if input.is_file() {
        return Ok(vec![input.to_path_buf()]);
    }
    if !input.is_dir() {
        bail!("input does not exist: {}", input.display());
    }

    let mut files = Vec::new();
    if recursive {
        for entry in WalkDir::new(input) {
            let entry = entry?;
            if entry.file_type().is_file() {
                files.push(entry.path().to_path_buf());
            }
        }
    } else {
        for entry in fs::read_dir(input)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                files.push(entry.path());
            }
        }
    }
    Ok(files)
}
