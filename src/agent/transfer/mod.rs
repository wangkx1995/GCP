pub mod backend;
pub mod config;
pub(crate) mod ftp;
pub(crate) mod sftp;

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

use crate::agent::transfer::backend::TransferBackend;
use crate::agent::transfer::config::TransferConfig;

type BackendFactory = Arc<dyn Fn() -> Result<Box<dyn TransferBackend + Send>> + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferSummary {
    pub package_count: usize,
    pub file_count: usize,
}

#[derive(Clone)]
pub struct OutputTransfer {
    config: TransferConfig,
    backend_factory: BackendFactory,
    dry_run: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputFile {
    pub local_path: PathBuf,
    pub relative_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputPackage {
    pub local_dir: PathBuf,
    pub remote_dir: String,
    pub files: Vec<OutputFile>,
}

fn safe_remote_component(value: &str) -> Result<&str> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || value.contains('\0')
    {
        bail!("unsafe output directory name: {value:?}");
    }
    Ok(value)
}

fn join_remote(base: &str, relative: &Path) -> Result<String> {
    let mut remote = base.trim_end_matches('/').to_string();
    for component in relative.components() {
        let std::path::Component::Normal(value) = component else {
            bail!("unsafe relative output path: {}", relative.display());
        };
        let value = value
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("relative output path is not valid UTF-8"))?;
        safe_remote_component(value)?;
        remote.push('/');
        remote.push_str(value);
    }
    Ok(remote)
}

pub fn upload_package_with_backend(
    backend: &mut dyn backend::TransferBackend,
    package: &OutputPackage,
) -> Result<()> {
    let remote_dir = package.remote_dir.trim_end_matches('/');
    let marker = format!("{remote_dir}/_SUCCESS");
    backend.remove_file_if_exists(&marker)?;
    backend.ensure_dir(remote_dir)?;

    for file in &package.files {
        let final_path = join_remote(remote_dir, &file.relative_path)?;
        let parent = final_path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or(remote_dir);
        backend.ensure_dir(parent)?;
        let part_path = format!("{final_path}.part");
        backend.remove_file_if_exists(&part_path)?;
        backend.upload_file(&file.local_path, &part_path)?;
        backend.rename_replace(&part_path, &final_path)?;
    }

    backend.create_empty_file(&marker)?;
    Ok(())
}

fn output_directory_name<'a>(value: &'a OsStr, level: &str) -> Result<&'a str> {
    value
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("{level} output directory name is not valid UTF-8"))
}

fn collect_output_files(
    local_dir: &Path,
    entries: impl IntoIterator<Item = walkdir::Result<DirEntry>>,
) -> Result<Vec<OutputFile>> {
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let local_path = entry.path().to_path_buf();
        let relative_path = local_path.strip_prefix(local_dir)?.to_path_buf();
        files.push(OutputFile {
            local_path,
            relative_path,
        });
    }
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(files)
}

pub fn discover_output_packages(
    output_dir: &Path,
    remote_prefix: &str,
) -> Result<Vec<OutputPackage>> {
    if !output_dir.exists() {
        return Ok(Vec::new());
    }

    let prefix = remote_prefix.trim_end_matches('/');
    let mut packages = Vec::new();

    for level1 in std::fs::read_dir(output_dir)? {
        let level1 = level1?;
        if !level1.file_type()?.is_dir() {
            continue;
        }
        let level1_file_name = level1.file_name();
        let level1_name = output_directory_name(&level1_file_name, "first-level")?;
        safe_remote_component(&level1_name)?;

        for level2 in std::fs::read_dir(level1.path())? {
            let level2 = level2?;
            if !level2.file_type()?.is_dir() {
                continue;
            }
            let level2_file_name = level2.file_name();
            let level2_name = output_directory_name(&level2_file_name, "second-level")?;
            safe_remote_component(&level2_name)?;
            let local_dir = level2.path();
            let files = collect_output_files(
                &local_dir,
                WalkDir::new(&local_dir).follow_links(false).min_depth(1),
            )?;
            packages.push(OutputPackage {
                local_dir,
                remote_dir: format!("{prefix}/{level1_name}/{level2_name}"),
                files,
            });
        }
    }

    packages.sort_by(|left, right| left.remote_dir.cmp(&right.remote_dir));
    Ok(packages)
}

impl OutputTransfer {
    pub fn new(config: TransferConfig, dry_run: bool) -> Self {
        let factory_config = config.clone();
        Self {
            config,
            backend_factory: Arc::new(move || backend::connect_backend(&factory_config)),
            dry_run,
        }
    }

    pub fn upload_output(&self, output_dir: &Path) -> Result<TransferSummary> {
        if !self.config.enabled {
            return Ok(TransferSummary {
                package_count: 0,
                file_count: 0,
            });
        }
        let packages = discover_output_packages(output_dir, &self.config.remote_prefix)?;
        if packages.is_empty() {
            return Ok(TransferSummary {
                package_count: 0,
                file_count: 0,
            });
        }
        if self.dry_run {
            for package in &packages {
                tracing::info!(
                    "[agent] dry-run: would upload package {} with {} file(s)",
                    package.remote_dir,
                    package.files.len(),
                );
            }
            let file_count = packages.iter().map(|package| package.files.len()).sum();
            return Ok(TransferSummary {
                package_count: packages.len(),
                file_count,
            });
        }
        let file_count = packages.iter().map(|package| package.files.len()).sum();
        for package in &packages {
            self.upload_package_with_retry(package)?;
        }
        Ok(TransferSummary {
            package_count: packages.len(),
            file_count,
        })
    }

    fn upload_package_with_retry(&self, package: &OutputPackage) -> Result<()> {
        let max_attempts = self.config.retry_count.max(1);
        let mut last_error = None;
        for attempt in 1..=max_attempts {
            let mut backend = (self.backend_factory)()?;
            match upload_package_with_backend(&mut *backend, package) {
                Ok(()) => return Ok(()),
                Err(error) => {
                    tracing::warn!(
                        "[agent] upload attempt {attempt}/{max_attempts} failed for {} (protocol={:?} host={} port={}): {error:#}",
                        package.remote_dir,
                        self.config.protocol,
                        self.config.host,
                        self.config.effective_port(),
                    );
                    last_error = Some(error);
                    if attempt < max_attempts {
                        std::thread::sleep(std::time::Duration::from_secs(
                            self.config.retry_interval_seconds,
                        ));
                    }
                }
            }
        }
        Err(last_error.unwrap()).context(format!("upload failed for {}", package.remote_dir))
    }

    #[cfg(test)]
    fn new_for_test(
        config: TransferConfig,
        backend_factory: BackendFactory,
        dry_run: bool,
    ) -> Self {
        Self {
            config,
            backend_factory,
            dry_run,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[cfg(unix)]
    use std::ffi::OsStr;

    #[cfg(unix)]
    use std::os::unix::ffi::OsStrExt;

    #[derive(Default)]
    struct RecordingBackend {
        operations: Vec<String>,
        fail_at: Option<usize>,
        created_markers: Vec<String>,
    }

    impl RecordingBackend {
        fn record(&mut self, operation: String) -> Result<()> {
            let operation_index = self.operations.len();
            self.operations.push(operation);
            if self.fail_at == Some(operation_index) {
                return Err(anyhow!("backend operation {operation_index} failed"));
            }
            Ok(())
        }
    }

    impl backend::TransferBackend for RecordingBackend {
        fn ensure_dir(&mut self, path: &str) -> Result<()> {
            self.record(format!("mkdir:{path}"))
        }

        fn remove_file_if_exists(&mut self, path: &str) -> Result<()> {
            self.record(format!("remove:{path}"))
        }

        fn upload_file(&mut self, local: &Path, remote: &str) -> Result<()> {
            self.record(format!("upload:{}:{remote}", local.display()))
        }

        fn rename_replace(&mut self, from: &str, to: &str) -> Result<()> {
            self.record(format!("rename:{from}:{to}"))
        }

        fn create_empty_file(&mut self, path: &str) -> Result<()> {
            self.record(format!("touch:{path}"))?;
            self.created_markers.push(path.to_string());
            Ok(())
        }
    }

    fn upload_test_package(package_dir: &Path) -> OutputPackage {
        OutputPackage {
            local_dir: package_dir.to_path_buf(),
            remote_dir: "/core/uploads/level1/package".to_string(),
            files: vec![
                OutputFile {
                    local_path: package_dir.join("a.csv"),
                    relative_path: PathBuf::from("a.csv"),
                },
                OutputFile {
                    local_path: package_dir.join("nested/a.ini"),
                    relative_path: PathBuf::from("nested/a.ini"),
                },
            ],
        }
    }

    #[test]
    fn uploads_package_atomically_and_creates_success_marker_last() {
        let dir = tempdir().unwrap();
        let package_dir = dir.path().join("package");
        std::fs::create_dir_all(package_dir.join("nested")).unwrap();
        std::fs::write(package_dir.join("a.csv"), b"a").unwrap();
        std::fs::write(package_dir.join("nested/a.ini"), b"i").unwrap();
        let package = upload_test_package(&package_dir);
        let mut backend = RecordingBackend::default();

        upload_package_with_backend(&mut backend, &package).unwrap();

        assert_eq!(
            backend.operations,
            vec![
                "remove:/core/uploads/level1/package/_SUCCESS",
                "mkdir:/core/uploads/level1/package",
                "mkdir:/core/uploads/level1/package",
                "remove:/core/uploads/level1/package/a.csv.part",
                &format!(
                    "upload:{}:/core/uploads/level1/package/a.csv.part",
                    package_dir.join("a.csv").display()
                ),
                "rename:/core/uploads/level1/package/a.csv.part:/core/uploads/level1/package/a.csv",
                "mkdir:/core/uploads/level1/package/nested",
                "remove:/core/uploads/level1/package/nested/a.ini.part",
                &format!(
                    "upload:{}:/core/uploads/level1/package/nested/a.ini.part",
                    package_dir.join("nested/a.ini").display()
                ),
                "rename:/core/uploads/level1/package/nested/a.ini.part:/core/uploads/level1/package/nested/a.ini",
                "touch:/core/uploads/level1/package/_SUCCESS",
            ]
        );
        assert_eq!(
            backend.created_markers,
            vec!["/core/uploads/level1/package/_SUCCESS"]
        );
    }

    #[test]
    fn every_backend_failure_aborts_without_creating_success_marker() {
        let dir = tempdir().unwrap();
        let package_dir = dir.path().join("package");
        let package = upload_test_package(&package_dir);

        for fail_at in 0..11 {
            let mut backend = RecordingBackend {
                fail_at: Some(fail_at),
                ..RecordingBackend::default()
            };

            let error = upload_package_with_backend(&mut backend, &package).unwrap_err();

            assert_eq!(
                error.to_string(),
                format!("backend operation {fail_at} failed")
            );
            assert!(backend.created_markers.is_empty(), "failure at {fail_at}");
            assert_eq!(backend.operations.len(), fail_at + 1);
        }
    }

    #[test]
    fn rejects_unsafe_relative_file_paths_before_uploading_them() {
        let dir = tempdir().unwrap();
        let package = OutputPackage {
            local_dir: dir.path().to_path_buf(),
            remote_dir: "/core/uploads/level1/package".to_string(),
            files: vec![OutputFile {
                local_path: dir.path().join("escape.csv"),
                relative_path: PathBuf::from("../escape.csv"),
            }],
        };
        let mut backend = RecordingBackend::default();

        let error = upload_package_with_backend(&mut backend, &package).unwrap_err();

        assert!(error.to_string().contains("unsafe relative output path"));
        assert_eq!(
            backend.operations,
            vec![
                "remove:/core/uploads/level1/package/_SUCCESS",
                "mkdir:/core/uploads/level1/package",
            ]
        );
        assert!(backend.created_markers.is_empty());
    }

    #[test]
    fn discovers_each_second_level_directory_as_independent_package() {
        let dir = tempdir().unwrap();
        let output = dir.path().join("output");
        let package_a = output
            .join("tpd_eutr_prb_q_2026061714")
            .join("LTE_PM_1604007_202606171445");
        let package_b = output
            .join("tpd_eutr_prb_q_2026061714")
            .join("LTE_PM_1604008_202606171445");
        std::fs::create_dir_all(package_a.join("nested")).unwrap();
        std::fs::create_dir_all(&package_b).unwrap();
        std::fs::write(package_a.join("a.csv"), b"a").unwrap();
        std::fs::write(package_a.join("nested/meta.ini"), b"m").unwrap();
        std::fs::write(package_b.join("b.csv"), b"b").unwrap();

        let packages = discover_output_packages(&output, "/core/uploads").unwrap();

        assert_eq!(packages.len(), 2);
        assert_eq!(
            packages[0].remote_dir,
            "/core/uploads/tpd_eutr_prb_q_2026061714/LTE_PM_1604007_202606171445"
        );
        assert_eq!(
            packages[0]
                .files
                .iter()
                .map(|file| file.relative_path.clone())
                .collect::<Vec<_>>(),
            vec![PathBuf::from("a.csv"), PathBuf::from("nested/meta.ini")]
        );
        assert_eq!(packages[0].local_dir, package_a);
        assert_eq!(packages[1].local_dir, package_b);
    }

    #[test]
    fn ignores_files_directly_under_output_and_first_level_directories() {
        let dir = tempdir().unwrap();
        let output = dir.path().join("output");
        std::fs::create_dir_all(output.join("level1")).unwrap();
        std::fs::write(output.join("orphan.txt"), b"x").unwrap();
        std::fs::write(output.join("level1/orphan.txt"), b"x").unwrap();

        let packages = discover_output_packages(&output, "/core/uploads").unwrap();

        assert!(packages.is_empty());
    }

    #[test]
    fn ignores_symlinks_in_package_tree() {
        #[cfg(unix)]
        {
            let dir = tempdir().unwrap();
            let output = dir.path().join("output");
            let package = output.join("level1/unique-package");
            std::fs::create_dir_all(&package).unwrap();
            std::fs::write(dir.path().join("outside.txt"), b"secret").unwrap();
            std::fs::create_dir_all(dir.path().join("outside-dir")).unwrap();
            std::fs::write(dir.path().join("outside-dir/secret.txt"), b"secret").unwrap();
            std::os::unix::fs::symlink(dir.path().join("outside.txt"), package.join("linked.txt"))
                .unwrap();
            std::os::unix::fs::symlink(dir.path().join("outside-dir"), package.join("linked-dir"))
                .unwrap();

            let packages = discover_output_packages(&output, "/core/uploads").unwrap();

            assert_eq!(packages.len(), 1);
            assert!(packages[0].files.is_empty());
        }
    }

    #[test]
    fn rejects_unsafe_directory_names() {
        for value in ["", ".", "..", "a/b", "a\\b", "a\0b"] {
            assert!(safe_remote_component(value).is_err(), "accepted {value:?}");
        }
        assert!(safe_remote_component("LTE_PM_1604007_202606171445").is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_non_utf8_first_level_directory_name() {
        let invalid_name = OsStr::from_bytes(b"level1-\xff");

        let error = output_directory_name(invalid_name, "first-level").unwrap_err();

        assert!(
            error
                .to_string()
                .contains("first-level output directory name is not valid UTF-8"),
            "unexpected error: {error:#}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_non_utf8_second_level_directory_name() {
        let invalid_name = OsStr::from_bytes(b"package-\xff");

        let error = output_directory_name(invalid_name, "second-level").unwrap_err();

        assert!(
            error
                .to_string()
                .contains("second-level output directory name is not valid UTF-8"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn propagates_walkdir_errors() {
        let dir = tempdir().unwrap();
        let local_dir = dir.path().join("package");
        let walk_error = WalkDir::new(dir.path().join("missing"))
            .into_iter()
            .next()
            .unwrap()
            .unwrap_err();

        let error = collect_output_files(&local_dir, [Err(walk_error)]).unwrap_err();

        let walk_error = error.downcast_ref::<walkdir::Error>().unwrap();
        assert_eq!(
            walk_error.io_error().unwrap().kind(),
            std::io::ErrorKind::NotFound
        );
    }

    #[test]
    fn returns_empty_when_output_directory_does_not_exist() {
        let dir = tempdir().unwrap();

        let packages =
            discover_output_packages(&dir.path().join("missing"), "/core/uploads").unwrap();

        assert!(packages.is_empty());
    }

    #[test]
    fn sorts_packages_and_files_stably() {
        let dir = tempdir().unwrap();
        let output = dir.path().join("output");
        let package_b = output.join("b-category/b-package");
        let package_a = output.join("a-category/a-package");
        std::fs::create_dir_all(&package_b).unwrap();
        std::fs::create_dir_all(package_a.join("nested")).unwrap();
        std::fs::write(package_a.join("z.txt"), b"z").unwrap();
        std::fs::write(package_a.join("a.txt"), b"a").unwrap();
        std::fs::write(package_a.join("nested/m.txt"), b"m").unwrap();

        let packages = discover_output_packages(&output, "/core/uploads/").unwrap();

        assert_eq!(
            packages
                .iter()
                .map(|package| package.remote_dir.as_str())
                .collect::<Vec<_>>(),
            vec![
                "/core/uploads/a-category/a-package",
                "/core/uploads/b-category/b-package"
            ]
        );
        assert_eq!(
            packages[0]
                .files
                .iter()
                .map(|file| file.relative_path.clone())
                .collect::<Vec<_>>(),
            vec![
                PathBuf::from("a.txt"),
                PathBuf::from("nested/m.txt"),
                PathBuf::from("z.txt")
            ]
        );
    }

    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FailingUploadBackend {
        attempts: Arc<AtomicUsize>,
        failures_before_success: usize,
    }

    impl backend::TransferBackend for FailingUploadBackend {
        fn ensure_dir(&mut self, _path: &str) -> Result<()> {
            Ok(())
        }
        fn remove_file_if_exists(&mut self, _path: &str) -> Result<()> {
            Ok(())
        }
        fn upload_file(&mut self, _local: &Path, _remote: &str) -> Result<()> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
            if attempt <= self.failures_before_success {
                anyhow::bail!("injected upload failure {attempt}");
            }
            Ok(())
        }
        fn rename_replace(&mut self, _from: &str, _to: &str) -> Result<()> {
            Ok(())
        }
        fn create_empty_file(&mut self, _path: &str) -> Result<()> {
            Ok(())
        }
    }

    fn retry_test_output() -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let output = dir.path().join("output");
        let package = output.join("level1/unique-package");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join("data.csv"), b"data").unwrap();
        (dir, output)
    }

    #[test]
    fn retries_failed_package_upload_up_to_configured_attempts() {
        let (_dir, output) = retry_test_output();
        let attempts = Arc::new(AtomicUsize::new(0));
        let factory_attempts = Arc::clone(&attempts);
        let transfer = OutputTransfer::new_for_test(
            TransferConfig {
                enabled: true,
                remote_prefix: "/core/uploads".to_string(),
                retry_count: 3,
                retry_interval_seconds: 0,
                ..TransferConfig::default()
            },
            Arc::new(move || {
                Ok(Box::new(FailingUploadBackend {
                    attempts: Arc::clone(&factory_attempts),
                    failures_before_success: 2,
                }))
            }),
            false,
        );

        let summary = transfer.upload_output(&output).unwrap();

        assert_eq!(summary.package_count, 1);
        assert_eq!(summary.file_count, 1);
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn returns_error_after_retry_count_is_exhausted() {
        let (_dir, output) = retry_test_output();
        let attempts = Arc::new(AtomicUsize::new(0));
        let factory_attempts = Arc::clone(&attempts);
        let transfer = OutputTransfer::new_for_test(
            TransferConfig {
                enabled: true,
                remote_prefix: "/core/uploads".to_string(),
                retry_count: 3,
                retry_interval_seconds: 0,
                ..TransferConfig::default()
            },
            Arc::new(move || {
                Ok(Box::new(FailingUploadBackend {
                    attempts: Arc::clone(&factory_attempts),
                    failures_before_success: usize::MAX,
                }))
            }),
            false,
        );

        let error = transfer.upload_output(&output).unwrap_err();

        assert!(error.to_string().contains("unique-package"));
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn empty_output_returns_success_without_creating_backend() {
        let dir = tempdir().unwrap();
        let output = dir.path().join("output");
        std::fs::create_dir_all(&output).unwrap();
        let factory_calls = Arc::new(AtomicUsize::new(0));
        let calls = Arc::clone(&factory_calls);
        let transfer = OutputTransfer::new_for_test(
            TransferConfig {
                enabled: true,
                remote_prefix: "/core/uploads".to_string(),
                ..TransferConfig::default()
            },
            Arc::new(move || {
                calls.fetch_add(1, Ordering::SeqCst);
                anyhow::bail!("backend must not be created for empty output")
            }),
            false,
        );

        let summary = transfer.upload_output(&output).unwrap();

        assert_eq!(
            summary,
            TransferSummary {
                package_count: 0,
                file_count: 0
            }
        );
        assert_eq!(factory_calls.load(Ordering::SeqCst), 0);
    }
}
