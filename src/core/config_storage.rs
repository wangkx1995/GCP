use std::io::Read;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use tracing::info;

const REQUIRED_FILES: &[&str] = &["source.toml", "mapping_dx.ini", "load.toml"];

#[derive(Debug)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
    pub config_snapshot_id: String,
    pub content_hash: String,
    pub file_count: usize,
}

#[derive(Clone, Debug)]
pub struct ConfigStorage {
    root: PathBuf,
}

impl ConfigStorage {
    pub fn new(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(root.join("versions"))?;
        info!("[config-storage] root={}", root.display());
        Ok(Self { root })
    }

    pub fn versions_dir(&self) -> PathBuf {
        self.root.join("versions")
    }

    pub fn active_link(&self) -> PathBuf {
        self.root.join("active")
    }

    pub fn version_dir(&self, snapshot_id: &str) -> PathBuf {
        self.versions_dir().join(snapshot_id)
    }

    pub fn validate_and_unpack(&self, zip_data: &[u8], snapshot_id: &str) -> Result<ValidationResult> {
        let mut errors: Vec<String> = Vec::new();
        let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
        let mut has_rules_dir = false;
        let mut total_entries: usize = 0;

        let reader = std::io::Cursor::new(zip_data);
        let mut archive = zip::ZipArchive::new(reader)
            .map_err(|e| anyhow::anyhow!("invalid zip: {e}"))?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let name = file.name().trim_end_matches('/').to_string();
            total_entries += 1;
            if file.is_dir() {
                if name == "rules" || name.starts_with("rules/") {
                    has_rules_dir = true;
                }
                continue;
            }
            let mut content = Vec::new();
            file.read_to_end(&mut content)?;
            entries.push((name, content));
        }

        let entry_names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();

        for required in REQUIRED_FILES {
            if !entry_names.contains(required) {
                errors.push(format!("missing required file: {required}"));
            }
        }
        if !has_rules_dir {
            let has_rule_file = entry_names.iter().any(|n| n.starts_with("rules/"));
            if !has_rule_file {
                errors.push("missing required directory: rules/".to_string());
            }
        }

        if !errors.is_empty() {
            return Ok(ValidationResult {
                valid: false,
                errors,
                config_snapshot_id: snapshot_id.to_string(),
                content_hash: String::new(),
                file_count: total_entries,
            });
        }

        // Sort entries by relative_path for deterministic hashing
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        // Compute hash
        let mut hasher = Sha256::new();
        for (name, content) in &entries {
            hasher.update(name.as_bytes());
            hasher.update(b"\0");
            hasher.update(content);
            hasher.update(b"\0");
        }
        let hash_value = hasher.finalize();
        let hash = format!("sha256:{}", hash_value.iter().map(|b| format!("{b:02x}")).collect::<String>());

        // Write to disk
        let version_dir = self.version_dir(snapshot_id);
        std::fs::create_dir_all(&version_dir)
            .with_context(|| format!("create version dir {}", version_dir.display()))?;

        for (name, content) in &entries {
            // Path traversal check (mirrors Agent's unpack_zip)
            let clean_name = name.replace('\\', "/");
            if clean_name.contains("..") || clean_name.starts_with('/') {
                anyhow::bail!("path traversal detected: {name}");
            }
            let target = version_dir.join(name);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("create parent dir {}", parent.display()))?;
            }
            std::fs::write(&target, content)
                .with_context(|| format!("write {}", target.display()))?;
        }

        info!("[config-storage] unpacked {} files to {}", entries.len(), version_dir.display());

        Ok(ValidationResult {
            valid: true,
            errors: Vec::new(),
            config_snapshot_id: snapshot_id.to_string(),
            content_hash: hash,
            file_count: total_entries,
        })
    }

    pub fn delete_version(&self, snapshot_id: &str) -> Result<()> {
        let vdir = self.version_dir(snapshot_id);
        if vdir.exists() {
            std::fs::remove_dir_all(&vdir)?;
        }
        Ok(())
    }
}

pub fn create_zip_from_dir(dir: &Path) -> Result<Vec<u8>> {
    let mut buf = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut buf);
    let options = zip::write::FileOptions::<'_, ()>::default();

    let entries = collect_files(dir, dir);
    for (rel_path, content) in &entries {
        let name = rel_path.to_string_lossy().replace('\\', "/");
        if content.is_empty() && name.ends_with('/') {
            zip.add_directory(&name, options).unwrap();
        } else {
            zip.start_file(&name, options).unwrap();
            use std::io::Write;
            zip.write_all(content).unwrap();
        }
    }
    zip.finish()?;
    Ok(buf.into_inner())
}

fn collect_files(base: &Path, dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let rel = path.strip_prefix(base).unwrap().to_path_buf();
            if path.is_dir() {
                result.push((rel.join(""), Vec::new()));
                result.extend(collect_files(base, &path));
            } else if path.is_file() {
                if let Ok(content) = std::fs::read(&path) {
                    result.push((rel, content));
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn make_valid_zip() -> Vec<u8> {
        let buf = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(buf);
        let opts = || zip::write::FileOptions::<'_, ()>::default();
        zip.add_directory("rules/", opts()).unwrap();
        zip.start_file("source.toml", opts()).unwrap();
        zip.write_all(b"[source]\ntype=\"sftp\"").unwrap();
        zip.start_file("mapping_dx.ini", opts()).unwrap();
        zip.write_all(b"[tableMapping]").unwrap();
        zip.start_file("load.toml", opts()).unwrap();
        zip.write_all(b"[clickhouse]").unwrap();
        zip.start_file("rules/rule_a.json", opts()).unwrap();
        zip.write_all(b"{\"table_name\":\"TPD_A\"}").unwrap();
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn validates_valid_zip() {
        let dir = tempdir().unwrap();
        let storage = ConfigStorage::new(dir.path().join("cs")).unwrap();
        let zip_data = make_valid_zip();
        let result = storage.validate_and_unpack(&zip_data, "v1_test").unwrap();
        assert!(result.valid);
        assert!(result.errors.is_empty());
        assert_eq!(result.file_count, 5);
        assert!(!result.content_hash.is_empty());
    }

    #[test]
    fn rejects_zip_missing_source_toml() {
        let dir = tempdir().unwrap();
        let storage = ConfigStorage::new(dir.path().join("cs")).unwrap();
        let buf = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(buf);
        let opts = || zip::write::FileOptions::<'_, ()>::default();
        zip.add_directory("rules/", opts()).unwrap();
        zip.start_file("mapping_dx.ini", opts()).unwrap();
        zip.write_all(b"[tableMapping]").unwrap();
        let zip_data = zip.finish().unwrap().into_inner();
        let result = storage.validate_and_unpack(&zip_data, "v2_test").unwrap();
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.contains("source.toml")));
    }

    #[test]
    fn rejects_zip_missing_rules_dir() {
        let dir = tempdir().unwrap();
        let storage = ConfigStorage::new(dir.path().join("cs")).unwrap();
        let buf = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(buf);
        let opts = || zip::write::FileOptions::<'_, ()>::default();
        zip.start_file("source.toml", opts()).unwrap();
        zip.write_all(b"[source]").unwrap();
        zip.start_file("mapping_dx.ini", opts()).unwrap();
        zip.write_all(b"[tableMapping]").unwrap();
        zip.start_file("load.toml", opts()).unwrap();
        zip.write_all(b"[clickhouse]").unwrap();
        let zip_data = zip.finish().unwrap().into_inner();
        let result = storage.validate_and_unpack(&zip_data, "v3_test").unwrap();
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.contains("rules")));
    }

    #[test]
    fn unpack_writes_files_to_disk() {
        let dir = tempdir().unwrap();
        let storage = ConfigStorage::new(dir.path().join("cs")).unwrap();
        let zip_data = make_valid_zip();
        let result = storage.validate_and_unpack(&zip_data, "v1_test").unwrap();
        assert!(result.valid);
        let vdir = storage.version_dir("v1_test");
        assert!(vdir.join("source.toml").exists());
        assert!(vdir.join("mapping_dx.ini").exists());
        assert!(vdir.join("load.toml").exists());
        assert!(vdir.join("rules").join("rule_a.json").exists());
    }
}
