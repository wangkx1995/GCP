use std::io::Read;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use tracing::info;

const REQUIRED_FILES: &[&str] = &["mapping_dx.ini"];

#[derive(Debug)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
    pub config_snapshot_id: String,
    pub content_hash: String,
    pub file_count: usize,
    pub table_names: Vec<String>,
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
        let mut table_names: Vec<String> = Vec::new();

        let reader = std::io::Cursor::new(zip_data);
        let mut archive = zip::ZipArchive::new(reader)
            .map_err(|e| anyhow::anyhow!("invalid zip: {e}"))?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let entry_name = file.name().trim_end_matches('/').to_string();
            total_entries += 1;
            // Skip macOS metadata entries (__MACOSX/ and ._ resource forks)
            if entry_name.starts_with("__MACOSX") || entry_name.contains("/._") || entry_name.starts_with("._") {
                continue;
            }
            // Skip .DS_Store files
            if entry_name.ends_with("/.DS_Store") || entry_name == ".DS_Store" {
                continue;
            }
            if file.is_dir() {
                if entry_name == "rules" || entry_name.ends_with("/rules") || entry_name.starts_with("rules/") {
                    has_rules_dir = true;
                }
                continue;
            }
            let mut content = Vec::new();
            file.read_to_end(&mut content)?;
            entries.push((entry_name, content));
        }

        // Detect and strip common top-level directory prefix before extracting table names
        let entry_names_raw: Vec<String> = entries.iter().map(|(n, _)| n.clone()).collect();
        tracing::debug!("[config_storage] zip raw entries ({total_entries} total): {:?}", entry_names_raw);

        let prefix = common_prefix_str(&entry_names_raw);
        if let Some(pfx) = &prefix {
            tracing::debug!("[config_storage] stripping common prefix: \"{pfx}/\"");
            for (name, _) in &mut entries {
                if let Some(stripped) = name.strip_prefix(&format!("{pfx}/")) {
                    *name = stripped.to_string();
                }
            }
        }

        let entry_names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();

        // Re-check has_rules_dir with normalized names
        if !has_rules_dir {
            has_rules_dir = entry_names.iter().any(|n| *n == "rules" || n.starts_with("rules/"));
        }

        // Extract table names from stripped entry names
        for name in &entry_names {
            if name.starts_with("rules/") && name.ends_with(".json") {
                let table = name
                    .trim_start_matches("rules/")
                    .trim_end_matches(".json");
                if !table.is_empty() && !table_names.contains(&table.to_string()) {
                    table_names.push(table.to_string());
                }
            }
        }

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
                table_names: Vec::new(),
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
            table_names,
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

/// Find the common top-level directory prefix among a list of file paths.
/// Returns `Some("dirname")` if all non-root entries share the same first path segment.
fn common_prefix_str(names: &[String]) -> Option<String> {
    let prefix = names.iter().find_map(|n| n.split('/').next()).filter(|s| !s.is_empty())?;
    if names.iter().all(|n| n == prefix || n.starts_with(&format!("{prefix}/"))) {
        Some(prefix.to_string())
    } else {
        None
    }
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
    fn rejects_zip_missing_mapping_dx_ini() {
        let dir = tempdir().unwrap();
        let storage = ConfigStorage::new(dir.path().join("cs")).unwrap();
        let buf = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(buf);
        let opts = || zip::write::FileOptions::<'_, ()>::default();
        zip.add_directory("rules/", opts()).unwrap();
        zip.start_file("source.toml", opts()).unwrap();
        zip.write_all(b"[source]").unwrap();
        let zip_data = zip.finish().unwrap().into_inner();
        let result = storage.validate_and_unpack(&zip_data, "v2_test").unwrap();
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.contains("mapping_dx.ini")));
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

    #[test]
    fn common_prefix_detects_directory_wrapper() {
        let names: Vec<String> = vec!["mycfg/mapping_dx.ini", "mycfg/rules/a.json", "mycfg/source.toml"].into_iter().map(String::from).collect();
        assert_eq!(common_prefix_str(&names), Some("mycfg".to_string()));
    }

    #[test]
    fn common_prefix_returns_none_for_flat() {
        let names: Vec<String> = vec!["mapping_dx.ini", "rules/a.json"].into_iter().map(String::from).collect();
        assert_eq!(common_prefix_str(&names), None);
    }

    #[test]
    fn common_prefix_returns_none_for_mixed() {
        let names: Vec<String> = vec!["a/mapping_dx.ini", "b/rules/a.json"].into_iter().map(String::from).collect();
        assert_eq!(common_prefix_str(&names), None);
    }

    #[test]
    fn validate_accepts_nested_zip() {
        let dir = tempdir().unwrap();
        let storage = ConfigStorage::new(dir.path().join("cs")).unwrap();
        let buf = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(buf);
        let opts = || zip::write::FileOptions::<'_, ()>::default();
        zip.start_file("mycfg/mapping_dx.ini", opts()).unwrap();
        zip.write_all(b"[tableMapping]\n").unwrap();
        zip.start_file("mycfg/rules/TPD_A.json", opts()).unwrap();
        zip.write_all(b"{}").unwrap();
        let zip_data = zip.finish().unwrap().into_inner();
        let result = storage.validate_and_unpack(&zip_data, "v_nested").unwrap();
        assert!(result.valid, "nested zip should be valid after prefix stripping");
    }

    #[test]
    fn validate_accepts_macos_created_zip() {
        let dir = tempdir().unwrap();
        let storage = ConfigStorage::new(dir.path().join("cs")).unwrap();
        let buf = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(buf);
        let opts = || zip::write::FileOptions::<'_, ()>::default();
        zip.add_directory("__MACOSX/", opts()).unwrap();
        zip.start_file("__MACOSX/._RNOP_V1.4.0_NR_GNB_DX_PM", opts()).unwrap();
        zip.write_all(b"\x00").unwrap();
        zip.start_file("RNOP_V1.4.0_NR_GNB_DX_PM/mapping_dx.ini", opts()).unwrap();
        zip.write_all(b"[tableMapping]\n").unwrap();
        zip.start_file("__MACOSX/RNOP_V1.4.0_NR_GNB_DX_PM/._mapping_dx.ini", opts()).unwrap();
        zip.write_all(b"\x00").unwrap();
        zip.add_directory("RNOP_V1.4.0_NR_GNB_DX_PM/rules/", opts()).unwrap();
        zip.start_file("RNOP_V1.4.0_NR_GNB_DX_PM/rules/TPD_A.json", opts()).unwrap();
        zip.write_all(b"{\"table_name\":\"TPD_A\"}").unwrap();
        zip.start_file("__MACOSX/RNOP_V1.4.0_NR_GNB_DX_PM/rules/._TPD_A.json", opts()).unwrap();
        zip.write_all(b"\x00").unwrap();
        let zip_data = zip.finish().unwrap().into_inner();
        let result = storage.validate_and_unpack(&zip_data, "v_macos").unwrap();
        assert!(result.valid, "macos-created zip should be valid after filtering __MACOSX and stripping prefix");
    }
}
