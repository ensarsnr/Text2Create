//! Artifact files (#13).
//!
//! The Project IR records each artifact's path, ownership, and last recorded
//! content (digest and size). This module stores the files themselves:
//!
//! ```text
//! prj_<uuid>/
//! ├── files/      artifact files; an artifact path is relative to this directory
//! └── .staging/   new content before it is moved into place
//! ```
//!
//! **Confinement.** Artifact paths are validated lexically by the IR
//! ([`ProjectPath`]) and checked again here. Files are opened one path
//! component at a time, relative to the handle of the directory that contains
//! it and with `O_NOFOLLOW`, so a symbolic link anywhere in the path, including
//! `files/` itself, is refused instead of followed. Checking and opening are
//! one operation, so a link created concurrently cannot redirect an access.
//! Artifact paths must therefore not contain symbolic links at all, even ones
//! that point inside the project. Existing files are never written through:
//! new content is written to a new file and moved into place, so a hard link
//! to a file outside the project cannot be used to change that file.
//!
//! **Order of operations.** A write first validates the change with the
//! Project IR write path, then writes the file, and records the content in
//! the database last. The content is recorded only after the file is in place,
//! and if recording fails, the previous file is put back:
//!
//! 1. The new content is written to `.staging/` and flushed to disk.
//! 2. It is moved into place in one step: with "no replace" semantics when the
//!    file must not exist, or by exchanging it with the current file when
//!    replacing. The previous content is checked after the exchange and moved
//!    back if it is not the expected content.
//! 3. The target directory and `.staging/` are flushed to disk, and the content
//!    record is saved. If either fails, the new file is removed or exchanged
//!    back, and the changed directories are flushed again, so the file and the
//!    record match again.
//!
//! **Crashes.** If the process stops between steps, `.staging/` may keep a file
//! named after the artifact. It is either unfinished new content or, after an
//! interrupted replacement, the previous content, whose digest is the recorded
//! one. Such files are never used as artifact content and are not removed
//! automatically. A crash after step 2 leaves new content that is not
//! recorded; [`OpenProject::check_artifacts`] reports it as changed, and
//! writing the same content again records it without rewriting the file.

use crate::{OpenProject, StoreError};
use rustix::fs::{AtFlags, CWD, FileType, Mode, OFlags, RenameFlags, Stat};
use rustix::io::Errno;
use sha2::Digest as _;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use text2create_ir::Applied;
use text2create_ir::change::{ChangeSet, Operation, Ref};
use text2create_ir::ids::ArtifactId;
use text2create_ir::model::{Actor, Artifact, InputRef, Ownership};
use text2create_ir::scalar::{Digest, ProjectPath};
use text2create_ir::source::{Clock, IdGenerator, Source};

/// Directory inside a project directory that holds the artifact files.
pub const FILES_DIRECTORY: &str = "files";

/// Directory inside a project directory for content that is not in place yet.
pub const STAGING_DIRECTORY: &str = ".staging";

/// Why an artifact file operation failed. Paths in messages are relative to
/// the project directory.
#[derive(Debug, thiserror::Error)]
pub enum ArtifactError {
    #[error("artifact `{0}` does not exist in the project")]
    UnknownArtifact(ArtifactId),
    #[error(
        "artifact path `{path}` contains a symbolic link at `{at}`; symbolic links are not followed"
    )]
    SymbolicLink { path: String, at: String },
    #[error("artifact path `{path}`: `{at}` is not a directory")]
    NotADirectory { path: String, at: String },
    #[error("artifact path `{path}` is not a regular file")]
    NotAFile { path: String },
    #[error("artifact file `{path}` does not exist")]
    Missing { path: String },
    #[error("artifact file `{path}` already exists and was not overwritten")]
    AlreadyExists { path: String },
    /// Only the user can replace the file of a user-owned artifact.
    #[error("artifact `{0}` is owned by the user; its file can only be replaced by the user")]
    UserOwned(ArtifactId),
    /// A replacement expected other recorded content than the project has.
    #[error("the recorded content of artifact `{artifact}` is not `{}`; reload the project", expected.as_str())]
    RecordMismatch {
        artifact: ArtifactId,
        expected: Digest,
    },
    /// The file differs from its recorded content, so it was changed outside
    /// Text2Create. It is never overwritten automatically.
    #[error(
        "artifact file `{path}` was changed outside Text2Create (recorded {}, found {}) and was not overwritten",
        recorded.as_str(),
        found.as_str()
    )]
    ExternallyModified {
        path: String,
        recorded: Digest,
        found: Digest,
    },
    /// Another artifact refers to the same file on a case-insensitive file
    /// system, such as the default macOS file system.
    #[error("artifact path `{path}` refers to the same file as the path of artifact `{other}`")]
    PathConflict { path: String, other: ArtifactId },
    #[error("permission denied for `{at}`: {source}")]
    PermissionDenied {
        at: String,
        #[source]
        source: io::Error,
    },
    #[error("file system error for `{at}`: {source}")]
    Io {
        at: String,
        #[source]
        source: io::Error,
    },
}

/// How [`OpenProject::write_artifact`] may treat an existing file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteMode {
    /// The file must not exist.
    Create,
    /// Replaces the file, which must still have the recorded content
    /// `expected`. The file of a user-owned artifact can only be replaced by
    /// the user.
    Replace { expected: Digest },
}

/// The content to write to an artifact's file.
#[derive(Clone, Debug)]
pub struct ArtifactWrite<'a> {
    pub artifact: ArtifactId,
    pub bytes: &'a [u8],
    pub mode: WriteMode,
    /// Records the content was derived from, for its provenance record.
    pub inputs: Vec<InputRef>,
}

/// The state of an artifact's file compared with its recorded content.
#[derive(Debug)]
pub enum ArtifactStatus {
    /// No content is recorded and there is no file.
    NotProduced,
    /// The file has the recorded content.
    Unchanged,
    /// Content is recorded, but the file does not exist.
    Missing,
    /// The file differs from the recorded content.
    Changed { digest: Digest, size_bytes: u64 },
    /// A file exists, but no content is recorded for it.
    Unrecorded { digest: Digest, size_bytes: u64 },
    /// The file could not be checked, for example because its path contains
    /// a symbolic link.
    Unreadable(ArtifactError),
}

/// The result of checking one artifact.
#[derive(Debug)]
pub struct ArtifactCheck {
    pub artifact: ArtifactId,
    pub path: ProjectPath,
    pub status: ArtifactStatus,
}

/// The content of an artifact's file as read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactFile {
    pub bytes: Vec<u8>,
    pub digest: Digest,
    /// Whether the file has the recorded content.
    pub matches_record: bool,
}

impl OpenProject {
    /// Writes an artifact's file and records its digest and size with
    /// `record_artifact_content` through the write path.
    ///
    /// The source is the producer of the content and is recorded in its
    /// provenance record. Nothing is recorded unless the file is in place, and
    /// on any error the file and the project are as before (see the module
    /// documentation for crashes). If the file already has exactly the new
    /// content, it is not rewritten, and only the record is added.
    pub fn write_artifact(
        &mut self,
        source: &Source,
        write: ArtifactWrite<'_>,
        clock: &mut dyn Clock,
        ids: &mut dyn IdGenerator,
    ) -> Result<Applied, StoreError> {
        let artifact = self.artifact(write.artifact)?.clone();
        let path = artifact.path.as_str();
        check_path_conflicts(self, &artifact)?;
        if let WriteMode::Replace { expected } = &write.mode {
            if artifact.content.as_ref().map(|c| &c.digest) != Some(expected) {
                return Err(ArtifactError::RecordMismatch {
                    artifact: artifact.id,
                    expected: expected.clone(),
                }
                .into());
            }
            if artifact.ownership == Ownership::User && !matches!(source.actor(), Actor::User {}) {
                return Err(ArtifactError::UserOwned(artifact.id).into());
            }
        }

        let (digest, size_bytes) = digest_bytes(write.bytes);
        let changes = ChangeSet {
            inputs: write.inputs,
            operations: vec![Operation::RecordArtifactContent {
                artifact: Ref::Id(artifact.id),
                digest: digest.clone(),
                size_bytes,
            }],
        };
        // Validated before any file is touched, so that only saving can fail
        // after the file is in place.
        let (next, applied) = self.prepare(source, &changes, clock, ids)?;

        let dirs = ProjectDirs::open(self.directory())?;
        let (parent, name) = parent_of(&dirs.files, path, true)?;
        let current = inspect(&parent, name, path)?;
        let placed = match (&write.mode, current) {
            (_, Some(file)) if file.digest == digest => Placed::Unchanged,
            (WriteMode::Create, Some(_)) => {
                return Err(ArtifactError::AlreadyExists {
                    path: files_path(path),
                }
                .into());
            }
            (WriteMode::Create, None) => dirs.create(&artifact, &parent, name, write.bytes)?,
            (WriteMode::Replace { .. }, None) => {
                return Err(ArtifactError::Missing {
                    path: files_path(path),
                }
                .into());
            }
            (WriteMode::Replace { expected }, Some(file)) if file.digest != *expected => {
                return Err(ArtifactError::ExternallyModified {
                    path: files_path(path),
                    recorded: expected.clone(),
                    found: file.digest,
                }
                .into());
            }
            (WriteMode::Replace { expected }, Some(file)) => {
                dirs.replace(&artifact, &parent, name, write.bytes, expected, &file.stat)?
            }
        };

        // Make both changed directories durable before the content is
        // recorded: the target, which has the new entry, and `.staging/`,
        // which lost it or holds the previous content.
        let saved = sync_dir(&parent, Dir::Target)
            .map_err(|e| io_error(files_path(path), e))
            .and_then(|()| {
                sync_dir(&dirs.staging, Dir::Staging)
                    .map_err(|e| io_error(STAGING_DIRECTORY.to_owned(), e))
            })
            .map_err(StoreError::from)
            .and_then(|()| self.commit(next, &applied));
        match saved {
            Ok(()) => {
                placed.finish(&dirs);
                Ok(applied)
            }
            Err(error) => {
                // Best effort: if putting the file back fails too, the file
                // differs from the record, which `check_artifacts` reports.
                // Undoing a replacement that the project did not record is
                // safe, because the previous file is the recorded content.
                let _ = placed.undo(&dirs, &parent, name);
                Err(error)
            }
        }
    }

    /// Reads an artifact's file. Symbolic links are not followed.
    pub fn read_artifact(&self, artifact: ArtifactId) -> Result<ArtifactFile, StoreError> {
        let artifact = self.artifact(artifact)?;
        let path = artifact.path.as_str();
        let files = open_files(self.directory(), false)?;
        let (parent, name) = parent_of(&files, path, false)?;
        let mut file = open_file(&parent, name, path)?.ok_or_else(|| ArtifactError::Missing {
            path: files_path(path),
        })?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|source| io_error(files_path(path), source))?;
        let (digest, _) = digest_bytes(&bytes);
        let matches_record = artifact.content.as_ref().map(|c| &c.digest) == Some(&digest);
        Ok(ArtifactFile {
            bytes,
            digest,
            matches_record,
        })
    }

    /// Compares every artifact's file with its recorded content. Changes
    /// nothing: differences are reported, never recorded or repaired.
    pub fn check_artifacts(&self) -> Vec<ArtifactCheck> {
        let files = open_files(self.directory(), false);
        self.project()
            .document()
            .artifacts
            .iter()
            .map(|artifact| {
                let path = artifact.path.as_str();
                let found = match &files {
                    Ok(files) => parent_of(files, path, false)
                        .and_then(|(parent, name)| inspect(&parent, name, path)),
                    // Reported for each artifact.
                    Err(_) => open_files(self.directory(), false).map(|_| None),
                };
                ArtifactCheck {
                    artifact: artifact.id,
                    path: artifact.path.clone(),
                    status: status(found, artifact),
                }
            })
            .collect()
    }

    fn artifact(&self, id: ArtifactId) -> Result<&Artifact, ArtifactError> {
        self.project()
            .document()
            .artifacts
            .iter()
            .find(|a| a.id == id)
            .ok_or(ArtifactError::UnknownArtifact(id))
    }
}

fn status(found: Result<Option<Inspected>, ArtifactError>, artifact: &Artifact) -> ArtifactStatus {
    match (found, &artifact.content) {
        (Err(ArtifactError::Missing { .. }) | Ok(None), None) => ArtifactStatus::NotProduced,
        (Err(ArtifactError::Missing { .. }) | Ok(None), Some(_)) => ArtifactStatus::Missing,
        (Err(error), _) => ArtifactStatus::Unreadable(error),
        (Ok(Some(file)), None) => ArtifactStatus::Unrecorded {
            digest: file.digest,
            size_bytes: file.size_bytes,
        },
        (Ok(Some(file)), Some(content)) if file.digest == content.digest => {
            ArtifactStatus::Unchanged
        }
        (Ok(Some(file)), Some(_)) => ArtifactStatus::Changed {
            digest: file.digest,
            size_bytes: file.size_bytes,
        },
    }
}

/// Refuses paths that name the same file as another artifact's path when
/// letter case is ignored, as on the default macOS file system.
fn check_path_conflicts(project: &OpenProject, artifact: &Artifact) -> Result<(), ArtifactError> {
    let folded = artifact.path.as_str().to_lowercase();
    let other = project
        .project()
        .document()
        .artifacts
        .iter()
        .find(|other| other.id != artifact.id && other.path.as_str().to_lowercase() == folded);
    match other {
        Some(other) => Err(ArtifactError::PathConflict {
            path: files_path(artifact.path.as_str()),
            other: other.id,
        }),
        None => Ok(()),
    }
}

// ------------------------------------------------------------ file handling

/// How the new content was put in place, to finish or undo it.
enum Placed {
    /// The file already had the content.
    Unchanged,
    /// A new file was created; `stat` identifies it.
    Created { stat: Stat },
    /// The file was exchanged; the previous content is at `.staging/<staged>`.
    Replaced { staged: String },
}

impl Placed {
    /// Removes the previous content after the record was saved.
    fn finish(self, dirs: &ProjectDirs) {
        if let Placed::Replaced { staged } = self {
            // A leftover previous file is harmless; see the module documentation.
            let _ = rustix::fs::unlinkat(&dirs.staging, staged.as_str(), AtFlags::empty());
            let _ = sync_dir(&dirs.staging, Dir::Staging);
        }
    }

    /// Restores the file as it was before the write.
    fn undo(self, dirs: &ProjectDirs, parent: &File, name: &str) -> io::Result<()> {
        match self {
            Placed::Unchanged => {}
            Placed::Created { stat } => {
                // Remove the file only if it is still the one created here.
                let now = rustix::fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)?;
                if (now.st_dev, now.st_ino) == (stat.st_dev, stat.st_ino) {
                    rustix::fs::unlinkat(parent, name, AtFlags::empty())?;
                }
            }
            Placed::Replaced { staged } => {
                rustix::fs::renameat_with(
                    &dirs.staging,
                    staged.as_str(),
                    parent,
                    name,
                    RenameFlags::EXCHANGE,
                )?;
                rustix::fs::unlinkat(&dirs.staging, staged.as_str(), AtFlags::empty())?;
                sync_dir(&dirs.staging, Dir::Staging)?;
            }
        }
        sync_dir(parent, Dir::Target)
    }
}

/// A directory that a write changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dir {
    /// The directory that contains the artifact file.
    Target,
    /// `.staging/`.
    Staging,
}

/// Flushes a directory's entries to disk.
fn sync_dir(dir: &File, which: Dir) -> io::Result<()> {
    #[cfg(test)]
    tests::sync_hook::record(which)?;
    #[cfg(not(test))]
    let _ = which;
    dir.sync_all()
}

/// Handles of the directories of one project. Holding handles instead of
/// paths keeps every access inside the directories that were checked.
struct ProjectDirs {
    files: File,
    staging: File,
}

impl ProjectDirs {
    /// Opens `files/` and `.staging/`, creating them if they are missing.
    fn open(directory: &Path) -> Result<Self, ArtifactError> {
        let project = open_project(directory)?;
        Ok(Self {
            files: open_dir(&project, FILES_DIRECTORY, true, FILES_DIRECTORY, "")?,
            staging: open_dir(&project, STAGING_DIRECTORY, true, STAGING_DIRECTORY, "")?,
        })
    }

    /// Creates the file `name` in `parent` with `bytes`; fails if it exists.
    fn create(
        &self,
        artifact: &Artifact,
        parent: &File,
        name: &str,
        bytes: &[u8],
    ) -> Result<Placed, ArtifactError> {
        let path = files_path(artifact.path.as_str());
        let (staged, file) = self.stage(artifact.id, bytes, None)?;
        let stat = rustix::fs::fstat(&file).map_err(|e| io_error(path.clone(), e.into()));
        drop(file);
        let moved = stat.and_then(|stat| {
            rustix::fs::renameat_with(
                &self.staging,
                staged.as_str(),
                parent,
                name,
                RenameFlags::NOREPLACE,
            )
            .map(|()| stat)
            .map_err(|e| match e {
                Errno::EXIST => ArtifactError::AlreadyExists { path: path.clone() },
                e => fs_error(e, &path),
            })
        });
        match moved {
            Ok(stat) => Ok(Placed::Created { stat }),
            Err(error) => {
                let _ = rustix::fs::unlinkat(&self.staging, staged.as_str(), AtFlags::empty());
                Err(error)
            }
        }
    }

    /// Exchanges the file `name` in `parent` with new content, if the current
    /// file still has the content `expected`. Otherwise it is exchanged back.
    fn replace(
        &self,
        artifact: &Artifact,
        parent: &File,
        name: &str,
        bytes: &[u8],
        expected: &Digest,
        current: &Stat,
    ) -> Result<Placed, ArtifactError> {
        let path = files_path(artifact.path.as_str());
        // Keep the permissions of the file, for example the executable bit.
        let mode = Mode::from_raw_mode(current.st_mode & 0o777);
        let (staged, file) = self.stage(artifact.id, bytes, Some(mode))?;
        drop(file);
        let exchange = |from: &File, from_name: &str, to: &File, to_name: &str| {
            rustix::fs::renameat_with(from, from_name, to, to_name, RenameFlags::EXCHANGE)
                .map_err(|e| fs_error(e, &path))
        };
        if let Err(error) = exchange(&self.staging, &staged, parent, name) {
            let _ = rustix::fs::unlinkat(&self.staging, staged.as_str(), AtFlags::empty());
            return Err(error);
        }
        // The previous content is now staged. Confirm that it is the expected
        // content, which closes the window between checking and exchanging.
        let previous = inspect(&self.staging, &staged, artifact.path.as_str());
        match previous {
            Ok(Some(file)) if file.digest == *expected => Ok(Placed::Replaced { staged }),
            other => {
                exchange(parent, name, &self.staging, &staged)?;
                let _ = rustix::fs::unlinkat(&self.staging, staged.as_str(), AtFlags::empty());
                Err(match other {
                    Ok(Some(file)) => ArtifactError::ExternallyModified {
                        path,
                        recorded: expected.clone(),
                        found: file.digest,
                    },
                    Ok(None) => ArtifactError::Missing { path },
                    Err(error) => error,
                })
            }
        }
    }

    /// Writes `bytes` to a new file in `.staging/` and flushes it to disk.
    /// On failure, no file is left behind.
    fn stage(
        &self,
        artifact: ArtifactId,
        bytes: &[u8],
        mode: Option<Mode>,
    ) -> Result<(String, File), ArtifactError> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let (name, mut file) = loop {
            let name = format!(
                "{artifact}-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW;
            match rustix::fs::openat(
                &self.staging,
                name.as_str(),
                flags | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o666),
            ) {
                Ok(fd) => break (name, File::from(fd)),
                Err(Errno::EXIST) => continue,
                Err(e) => return Err(fs_error(e, STAGING_DIRECTORY)),
            }
        };
        let at = format!("{STAGING_DIRECTORY}/{name}");
        let written = file
            .write_all(bytes)
            .and_then(|()| match mode {
                Some(mode) => rustix::fs::fchmod(&file, mode).map_err(io::Error::from),
                None => Ok(()),
            })
            .and_then(|()| file.sync_all());
        match written {
            Ok(()) => Ok((name, file)),
            Err(error) => {
                drop(file);
                let _ = rustix::fs::unlinkat(&self.staging, name.as_str(), AtFlags::empty());
                Err(io_error(at, error))
            }
        }
    }
}

fn open_project(directory: &Path) -> Result<File, ArtifactError> {
    rustix::fs::openat(CWD, directory, dir_flags(), Mode::empty())
        .map(File::from)
        .map_err(|e| io_error(directory.display().to_string(), e.into()))
}

/// Opens `files/`, creating it if `create` is set.
fn open_files(directory: &Path, create: bool) -> Result<File, ArtifactError> {
    open_dir(
        &open_project(directory)?,
        FILES_DIRECTORY,
        create,
        FILES_DIRECTORY,
        "",
    )
}

/// Opens the directory in `files` that contains `path` and returns it with
/// the file name. Missing directories are created if `create` is set.
fn parent_of<'p>(
    files: &File,
    path: &'p str,
    create: bool,
) -> Result<(File, &'p str), ArtifactError> {
    let components = components(path)?;
    let (name, parents) = components.split_last().expect("at least one component");
    let mut current = files
        .try_clone()
        .map_err(|e| io_error(FILES_DIRECTORY.to_owned(), e))?;
    let mut at = FILES_DIRECTORY.to_owned();
    for component in parents {
        at = format!("{at}/{component}");
        current = open_dir(&current, component, create, &at, path)?;
    }
    Ok((current, name))
}

/// A file's content digest, size, and metadata.
struct Inspected {
    digest: Digest,
    size_bytes: u64,
    stat: Stat,
}

/// Opens the regular file `name` in `parent` for reading, or `None` if it
/// does not exist. Symbolic links are refused.
fn open_file(parent: &File, name: &str, path: &str) -> Result<Option<File>, ArtifactError> {
    // NONBLOCK: opening a FIFO must not wait for a writer.
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let at = files_path(path);
    let file = match rustix::fs::openat(parent, name, flags, Mode::empty()) {
        Ok(fd) => File::from(fd),
        Err(Errno::NOENT) => return Ok(None),
        Err(e) => return Err(classify(parent, name, e, path, &at)),
    };
    let stat = rustix::fs::fstat(&file).map_err(|e| io_error(at.clone(), e.into()))?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
        return Err(ArtifactError::NotAFile { path: at });
    }
    Ok(Some(file))
}

/// The digest and size of the file `name` in `parent`, or `None` if it does
/// not exist.
fn inspect(parent: &File, name: &str, path: &str) -> Result<Option<Inspected>, ArtifactError> {
    let Some(mut file) = open_file(parent, name, path)? else {
        return Ok(None);
    };
    let stat = rustix::fs::fstat(&file).map_err(|e| io_error(files_path(path), e.into()))?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    let mut size_bytes = 0u64;
    loop {
        let read = match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(io_error(files_path(path), e)),
        };
        hasher.update(&buffer[..read]);
        size_bytes += read as u64;
    }
    Ok(Some(Inspected {
        digest: hex_digest(&hasher.finalize()),
        size_bytes,
        stat,
    }))
}

/// Opens the directory `name` in `parent` without following a symbolic link,
/// creating it first if it is missing and `create` is set.
fn open_dir(
    parent: &File,
    name: &str,
    create: bool,
    at: &str,
    path: &str,
) -> Result<File, ArtifactError> {
    let open = || rustix::fs::openat(parent, name, dir_flags(), Mode::empty()).map(File::from);
    let result = match open() {
        Err(Errno::NOENT) if create => {
            match rustix::fs::mkdirat(parent, name, Mode::from_raw_mode(0o777)) {
                Ok(()) | Err(Errno::EXIST) => open(),
                Err(e) => Err(e),
            }
        }
        result => result,
    };
    result.map_err(|e| match e {
        Errno::NOENT => ArtifactError::Missing {
            path: files_path(path),
        },
        e => classify(parent, name, e, path, at),
    })
}

/// Turns an error from opening `name` in `parent` into an artifact error,
/// identifying symbolic links and non-directories.
fn classify(parent: &File, name: &str, error: Errno, path: &str, at: &str) -> ArtifactError {
    // `O_NOFOLLOW` reports a link as ELOOP, or EMLINK on some systems.
    let link_or_file = matches!(error, Errno::LOOP | Errno::NOTDIR | Errno::MLINK);
    let found = rustix::fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW);
    match found.map(|stat| FileType::from_raw_mode(stat.st_mode)) {
        Ok(FileType::Symlink) if link_or_file => ArtifactError::SymbolicLink {
            path: files_path(path),
            at: at.to_owned(),
        },
        Ok(kind) if kind != FileType::Directory && error == Errno::NOTDIR => {
            ArtifactError::NotADirectory {
                path: files_path(path),
                at: at.to_owned(),
            }
        }
        _ => fs_error(error, at),
    }
}

fn fs_error(error: Errno, at: &str) -> ArtifactError {
    match error {
        Errno::ACCESS | Errno::PERM | Errno::ROFS => ArtifactError::PermissionDenied {
            at: at.to_owned(),
            source: error.into(),
        },
        _ => io_error(at.to_owned(), error.into()),
    }
}

fn io_error(at: String, source: io::Error) -> ArtifactError {
    match source.kind() {
        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => {
            ArtifactError::PermissionDenied { at, source }
        }
        _ => ArtifactError::Io { at, source },
    }
}

fn dir_flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}

/// The components of an artifact path. [`ProjectPath`] already guarantees
/// these rules; they are checked again because a mistake here would let a
/// path leave the project directory.
fn components(path: &str) -> Result<Vec<&str>, ArtifactError> {
    let components: Vec<&str> = path.split('/').collect();
    let valid = !path.starts_with('/')
        && !path.contains('\\')
        && components
            .iter()
            .all(|c| !c.is_empty() && *c != "." && *c != ".." && !c.contains('\0'));
    if valid {
        Ok(components)
    } else {
        Err(ArtifactError::Io {
            at: path.to_owned(),
            source: io::Error::new(io::ErrorKind::InvalidInput, "invalid artifact path"),
        })
    }
}

/// The path of an artifact file relative to the project directory.
fn files_path(path: &str) -> String {
    format!("{FILES_DIRECTORY}/{path}")
}

fn digest_bytes(bytes: &[u8]) -> (Digest, u64) {
    (hex_digest(&sha2::Sha256::digest(bytes)), bytes.len() as u64)
}

fn hex_digest(hash: &[u8]) -> Digest {
    use std::fmt::Write as _;
    let mut text = String::from("sha256:");
    for byte in hash {
        write!(text, "{byte:02x}").expect("writing to a string");
    }
    Digest::try_from(text).expect("a SHA-256 digest is valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProjectStore;
    use std::path::PathBuf;
    use std::sync::Arc;
    use text2create_ir::domain::DomainRegistry;
    use text2create_ir::scalar::{Text, Timestamp};

    /// Records directory flushes and fails one on request, per thread.
    pub(super) mod sync_hook {
        use super::Dir;
        use std::cell::{Cell, RefCell};
        use std::io;

        thread_local! {
            static LOG: RefCell<Vec<Dir>> = const { RefCell::new(Vec::new()) };
            static FAIL: Cell<Option<Dir>> = const { Cell::new(None) };
        }

        pub fn record(which: Dir) -> io::Result<()> {
            LOG.with(|log| log.borrow_mut().push(which));
            if FAIL.with(|fail| fail.get()) == Some(which) {
                FAIL.with(|fail| fail.set(None));
                return Err(io::Error::other("injected flush failure"));
            }
            Ok(())
        }

        /// Clears the log; the next flush of `fail` fails once.
        pub fn reset(fail: Option<Dir>) {
            LOG.with(|log| log.borrow_mut().clear());
            FAIL.with(|f| f.set(fail));
        }

        pub fn log() -> Vec<Dir> {
            LOG.with(|log| log.borrow().clone())
        }
    }

    struct TestClock(u32);

    impl Clock for TestClock {
        fn now(&mut self) -> Timestamp {
            self.0 += 1;
            Timestamp::try_from(format!("2026-11-28T11:00:{:02}.000Z", self.0)).unwrap()
        }
    }

    struct TestIds(u128);

    impl IdGenerator for TestIds {
        fn next_uuid(&mut self) -> uuid::Uuid {
            self.0 += 1;
            uuid::Uuid::from_u128(0x0193_0000_0000_7000_8000_0000_0000_0000 + self.0)
        }
    }

    /// An open project with one generated artifact `a/out.txt`, in a
    /// temporary directory that is removed when dropped.
    struct Fixture {
        root: PathBuf,
        project: OpenProject,
        artifact: ArtifactId,
        clock: TestClock,
        ids: TestIds,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "text2create-sync-test-{}-{name}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            let (mut clock, mut ids) = (TestClock(0), TestIds(0));
            let store = ProjectStore::new(root.clone(), Arc::new(DomainRegistry::new()));
            let mut project = store
                .create(
                    &Source::user_edit(),
                    Text::try_from("Sync".to_owned()).unwrap(),
                    None,
                    &mut clock,
                    &mut ids,
                )
                .unwrap();
            let changes = ChangeSet::from_json(
                br#"{"inputs": [], "operations": [{"op": "add_artifact", "kind": "core.document",
                "path": "a/out.txt", "ownership": "generated"}]}"#,
            )
            .unwrap();
            project
                .apply(&Source::user_edit(), &changes, &mut clock, &mut ids)
                .unwrap();
            let artifact = project.project().document().artifacts[0].id;
            Self {
                root,
                project,
                artifact,
                clock,
                ids,
            }
        }

        fn write(&mut self, bytes: &[u8], mode: WriteMode) -> Result<Applied, StoreError> {
            let write = ArtifactWrite {
                artifact: self.artifact,
                bytes,
                mode,
                inputs: Vec::new(),
            };
            self.project
                .write_artifact(&Source::user_edit(), write, &mut self.clock, &mut self.ids)
        }

        fn replace(&mut self, bytes: &[u8]) -> Result<Applied, StoreError> {
            let document = self.project.project().document();
            let expected = document.artifacts[0]
                .content
                .as_ref()
                .unwrap()
                .digest
                .clone();
            self.write(bytes, WriteMode::Replace { expected })
        }

        fn file(&self) -> Option<Vec<u8>> {
            std::fs::read(self.project.directory().join("files/a/out.txt")).ok()
        }

        fn staged(&self) -> usize {
            std::fs::read_dir(self.project.directory().join(STAGING_DIRECTORY))
                .unwrap()
                .count()
        }

        /// The stored revision and document, read with a separate connection.
        fn stored(&self) -> (i64, String) {
            rusqlite::Connection::open(self.project.directory().join(crate::DATABASE_FILE))
                .unwrap()
                .query_row("SELECT revision, document FROM project_ir", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn create_flushes_the_target_and_staging_directories() {
        let mut fixture = Fixture::new("create");
        sync_hook::reset(None);
        fixture.write(b"one", WriteMode::Create).unwrap();
        assert_eq!(sync_hook::log(), [Dir::Target, Dir::Staging]);
    }

    #[test]
    fn replace_flushes_the_target_and_staging_directories() {
        let mut fixture = Fixture::new("replace");
        fixture.write(b"one", WriteMode::Create).unwrap();
        sync_hook::reset(None);
        fixture.replace(b"two").unwrap();
        // Both before saving, and `.staging/` again after the previous
        // content was removed.
        assert_eq!(sync_hook::log(), [Dir::Target, Dir::Staging, Dir::Staging]);
        assert_eq!(fixture.file().unwrap(), b"two");
    }

    #[test]
    fn a_failed_flush_records_nothing_and_restores_the_file() {
        for failing in [Dir::Target, Dir::Staging] {
            // Create: the new file is removed again.
            let mut fixture = Fixture::new(&format!("create-{failing:?}"));
            let (stored, document) = (fixture.stored(), fixture.project.project().clone());
            sync_hook::reset(Some(failing));
            let error = fixture.write(b"one", WriteMode::Create).unwrap_err();
            assert!(
                matches!(error, StoreError::Artifact(ArtifactError::Io { .. })),
                "{error}"
            );
            assert_eq!(fixture.stored(), stored);
            assert_eq!(fixture.project.project(), &document);
            assert_eq!(fixture.file(), None);
            assert_eq!(fixture.staged(), 0);
            // The undo flushes the directory it changed.
            let log = sync_hook::log();
            assert_eq!(log.last(), Some(&Dir::Target), "{failing:?}: {log:?}");

            // Replace: the previous content is exchanged back.
            let mut fixture = Fixture::new(&format!("replace-{failing:?}"));
            fixture.write(b"one", WriteMode::Create).unwrap();
            let (stored, document) = (fixture.stored(), fixture.project.project().clone());
            sync_hook::reset(Some(failing));
            let error = fixture.replace(b"two").unwrap_err();
            assert!(
                matches!(error, StoreError::Artifact(ArtifactError::Io { .. })),
                "{error}"
            );
            assert_eq!(fixture.stored(), stored);
            assert_eq!(fixture.project.project(), &document);
            assert_eq!(fixture.file().unwrap(), b"one");
            assert_eq!(fixture.staged(), 0);
            // The undo flushes both directories it changed.
            let log = sync_hook::log();
            assert_eq!(
                &log[log.len() - 2..],
                [Dir::Staging, Dir::Target],
                "{failing:?}: {log:?}"
            );
            sync_hook::reset(None);
        }
    }

    #[test]
    fn components_outside_the_project_are_refused() {
        for bad in [
            "/etc/passwd",
            "../x",
            "a/../../x",
            "a/./b",
            "a//b",
            "",
            "a\\b",
            "a/",
        ] {
            assert!(components(bad).is_err(), "{bad:?}");
        }
        assert_eq!(components("a/b c/d.txt").unwrap(), ["a", "b c", "d.txt"]);
    }

    #[test]
    fn digests_are_sha256() {
        let (digest, size) = digest_bytes(b"abc");
        assert_eq!(
            digest.as_str(),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(size, 3);
    }
}
