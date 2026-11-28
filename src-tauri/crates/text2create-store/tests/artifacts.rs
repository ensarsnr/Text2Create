//! Artifact files in temporary project directories (#13).

use rusqlite::Connection;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use text2create_ir::change::ChangeSet;
use text2create_ir::domain::DomainRegistry;
use text2create_ir::ids::{ArtifactId, ProjectId};
use text2create_ir::model::{Activity, Actor};
use text2create_ir::scalar::{Digest, Text, Timestamp};
use text2create_ir::source::{Clock, IdGenerator, Source};
use text2create_store::{
    ArtifactError, ArtifactStatus, ArtifactWrite, DATABASE_FILE, FILES_DIRECTORY, OpenProject,
    ProjectStore, STAGING_DIRECTORY, StoreError, WriteMode,
};

// ------------------------------------------------------------------ helpers

/// A temporary directory removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "text2create-artifact-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Restore permissions changed by a test so that removal succeeds.
        let _ = std::process::Command::new("chmod")
            .args(["-R", "u+rwx"])
            .arg(&self.0)
            .status();
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct TestClock(u32);

impl Clock for TestClock {
    fn now(&mut self) -> Timestamp {
        self.0 += 1;
        Timestamp::try_from(format!(
            "2026-11-28T10:{:02}:{:02}.000Z",
            self.0 / 60,
            self.0 % 60
        ))
        .unwrap()
    }
}

struct TestIds(u128);

impl IdGenerator for TestIds {
    fn next_uuid(&mut self) -> uuid::Uuid {
        self.0 += 1;
        uuid::Uuid::from_u128(0x0193_0000_0000_7000_8000_0000_0000_0000 + self.0)
    }
}

struct Env {
    dir: TempDir,
    clock: TestClock,
    ids: TestIds,
    project: Option<OpenProject>,
}

impl Env {
    /// A new project, open.
    fn new() -> Self {
        let mut env = Self {
            dir: TempDir::new(),
            clock: TestClock(0),
            ids: TestIds(0),
            project: None,
        };
        let project = env
            .store()
            .create(
                &Source::user_edit(),
                Text::try_from("Artifacts".to_owned()).unwrap(),
                None,
                &mut env.clock,
                &mut env.ids,
            )
            .unwrap();
        env.project = Some(project);
        env
    }

    fn store(&self) -> ProjectStore {
        ProjectStore::new(self.dir.0.join("projects"), Arc::new(DomainRegistry::new()))
    }

    fn project(&mut self) -> &mut OpenProject {
        self.project.as_mut().unwrap()
    }

    fn id(&mut self) -> ProjectId {
        self.project().id()
    }

    fn directory(&mut self) -> PathBuf {
        self.project().directory().to_owned()
    }

    fn files(&mut self) -> PathBuf {
        self.directory().join(FILES_DIRECTORY)
    }

    /// Closes the project and opens it again with a new store, as after a
    /// restart.
    fn reopen(&mut self) {
        let id = self.id();
        self.project.take().unwrap().close().unwrap();
        self.project = Some(self.store().open(id).unwrap());
    }

    /// Adds an artifact reference without content.
    fn add(&mut self, path: &str, ownership: &str) -> ArtifactId {
        let json = format!(
            r#"{{"inputs": [], "operations": [{{"op": "add_artifact", "kind": "core.document",
            "path": "{path}", "ownership": "{ownership}"}}]}}"#
        );
        let changes = ChangeSet::from_json(json.as_bytes()).unwrap();
        let (clock, ids) = (&mut self.clock, &mut self.ids);
        let project = self.project.as_mut().unwrap();
        project
            .apply(&Source::user_edit(), &changes, clock, ids)
            .unwrap();
        project.project().document().artifacts.last().unwrap().id
    }

    fn write(
        &mut self,
        source: &Source,
        artifact: ArtifactId,
        bytes: &[u8],
        mode: WriteMode,
    ) -> Result<(), StoreError> {
        let (clock, ids) = (&mut self.clock, &mut self.ids);
        self.project
            .as_mut()
            .unwrap()
            .write_artifact(
                source,
                ArtifactWrite {
                    artifact,
                    bytes,
                    mode,
                    inputs: Vec::new(),
                },
                clock,
                ids,
            )
            .map(|_| ())
    }

    fn create(&mut self, artifact: ArtifactId, bytes: &[u8]) -> Result<(), StoreError> {
        self.write(&Source::user_edit(), artifact, bytes, WriteMode::Create)
    }

    fn recorded(&mut self, artifact: ArtifactId) -> Option<Digest> {
        let document = self.project().project().document();
        let artifact = document
            .artifacts
            .iter()
            .find(|a| a.id == artifact)
            .unwrap();
        artifact.content.as_ref().map(|c| c.digest.clone())
    }

    fn status(&mut self, artifact: ArtifactId) -> ArtifactStatus {
        self.project()
            .check_artifacts()
            .into_iter()
            .find(|c| c.artifact == artifact)
            .unwrap()
            .status
    }

    /// The stored document and revision, read with a separate connection.
    fn stored(&mut self) -> (String, i64) {
        let path = self.directory().join(DATABASE_FILE);
        Connection::open(path)
            .unwrap()
            .query_row("SELECT document, revision FROM project_ir", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap()
    }

    /// The names in `.staging/`.
    fn staged(&mut self) -> Vec<String> {
        match fs::read_dir(self.directory().join(STAGING_DIRECTORY)) {
            Ok(entries) => entries
                .map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect(),
            Err(_) => Vec::new(),
        }
    }
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    let hash = sha2::Sha256::digest(bytes);
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

fn digest(bytes: &[u8]) -> Digest {
    Digest::try_from(sha256(bytes)).unwrap()
}

/// A source that is not the user.
fn generator() -> Source {
    Source::system_import(Text::try_from("generator".to_owned()).unwrap())
}

#[track_caller]
fn assert_artifact_error(result: Result<(), StoreError>, check: impl Fn(&ArtifactError) -> bool) {
    match result {
        Err(StoreError::Artifact(error)) => assert!(check(&error), "unexpected error: {error}"),
        other => panic!("expected an artifact error, got {other:?}"),
    }
}

// -------------------------------------------------------------------- tests

#[test]
fn an_artifact_is_written_recorded_and_read_after_reopening() {
    let mut env = Env::new();
    let artifact = env.add("docs/firmware/wiring.md", "generated");
    let content = b"# Wiring\n\nGPIO 4: sensor data.\n";
    let before = env.stored().1;
    env.write(&generator(), artifact, content, WriteMode::Create)
        .unwrap();

    // The file is in the nested directories under files/.
    let path = env.files().join("docs/firmware/wiring.md");
    assert_eq!(fs::read(&path).unwrap(), content);
    assert!(env.staged().is_empty(), "{:?}", env.staged());

    // The record has the real digest and size and names the producer.
    let document = env.project().project().document().clone();
    let record = document.artifacts[0].content.as_ref().unwrap();
    assert_eq!(record.digest.as_str(), sha256(content));
    assert_eq!(record.size_bytes, content.len() as u64);
    assert_eq!(document.artifacts[0].provenance.rev, 2);
    assert_eq!(document.artifacts[0].provenance.modified, record.provenance);
    let provenance = document
        .provenance
        .iter()
        .find(|p| p.id == record.provenance)
        .unwrap();
    assert!(matches!(provenance.actor, Actor::System { .. }));
    assert_eq!(provenance.activity, Activity::Import {});
    assert_eq!(env.stored().1, before + 1);

    env.reopen();
    assert_eq!(env.project().project().document(), &document);
    let file = env.project().read_artifact(artifact).unwrap();
    assert_eq!(
        (
            file.bytes.as_slice(),
            file.digest.as_str(),
            file.matches_record
        ),
        (&content[..], sha256(content).as_str(), true)
    );
    assert!(matches!(env.status(artifact), ArtifactStatus::Unchanged));
}

#[test]
fn a_generated_file_is_replaced_only_with_its_recorded_content_expected() {
    let mut env = Env::new();
    let artifact = env.add("main.c", "generated");
    env.create(artifact, b"int main(void) { return 0; }\n")
        .unwrap();
    let path = env.files().join("main.c");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o750)).unwrap();

    // Create never overwrites.
    assert_artifact_error(
        env.create(artifact, b"other"),
        |e| matches!(e, ArtifactError::AlreadyExists { path } if path == "files/main.c"),
    );
    // A replacement must name the recorded content.
    let stale = WriteMode::Replace {
        expected: digest(b"something else"),
    };
    assert_artifact_error(env.write(&generator(), artifact, b"x", stale), |e| {
        matches!(e, ArtifactError::RecordMismatch { .. })
    });

    let expected = env.recorded(artifact).unwrap();
    env.write(
        &generator(),
        artifact,
        b"v2\n",
        WriteMode::Replace { expected },
    )
    .unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"v2\n");
    assert_eq!(env.recorded(artifact).unwrap(), digest(b"v2\n"));
    // The permissions of the replaced file are kept.
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o750
    );
    assert!(env.staged().is_empty(), "{:?}", env.staged());
}

#[test]
fn user_owned_files_are_replaced_only_by_the_user() {
    let mut env = Env::new();
    let artifact = env.add("notes.txt", "user");
    env.create(artifact, b"my notes").unwrap();
    let expected = env.recorded(artifact).unwrap();
    let before = env.stored();

    let replace = WriteMode::Replace {
        expected: expected.clone(),
    };
    assert_artifact_error(
        env.write(&generator(), artifact, b"generated", replace.clone()),
        |e| matches!(e, ArtifactError::UserOwned(id) if *id == artifact),
    );
    assert_eq!(
        fs::read(env.files().join("notes.txt")).unwrap(),
        b"my notes"
    );
    assert_eq!(env.stored(), before);

    env.write(&Source::user_edit(), artifact, b"edited", replace)
        .unwrap();
    assert_eq!(fs::read(env.files().join("notes.txt")).unwrap(), b"edited");
}

#[test]
fn an_existing_file_that_was_never_recorded_is_not_overwritten() {
    let mut env = Env::new();
    let artifact = env.add("report.md", "generated");
    fs::create_dir_all(env.files()).unwrap();
    fs::write(env.files().join("report.md"), b"written by hand").unwrap();
    assert!(matches!(
        env.status(artifact),
        ArtifactStatus::Unrecorded { size_bytes: 15, .. }
    ));
    let before = env.stored();
    assert_artifact_error(env.create(artifact, b"generated"), |e| {
        matches!(e, ArtifactError::AlreadyExists { .. })
    });
    assert_eq!(
        fs::read(env.files().join("report.md")).unwrap(),
        b"written by hand"
    );
    assert_eq!(env.stored(), before);
    assert!(env.staged().is_empty());

    // Writing exactly the existing content records it without rewriting.
    let inode = |path: &Path| std::os::unix::fs::MetadataExt::ino(&fs::metadata(path).unwrap());
    let path = env.files().join("report.md");
    let old = inode(&path);
    env.create(artifact, b"written by hand").unwrap();
    assert_eq!(inode(&path), old);
    assert!(matches!(env.status(artifact), ArtifactStatus::Unchanged));
}

#[test]
fn traversal_and_absolute_paths_are_rejected_by_the_write_path() {
    let mut env = Env::new();
    let before = env.stored();
    for path in [
        "../outside.txt",
        "docs/../../outside.txt",
        "/etc/passwd",
        "./a",
        "a//b",
        "a\\b",
    ] {
        let json = format!(
            r#"{{"inputs": [], "operations": [{{"op": "add_artifact", "kind": "core.document",
            "path": {}, "ownership": "generated"}}]}}"#,
            serde_json::to_string(path).unwrap()
        );
        assert!(
            ChangeSet::from_json(json.as_bytes()).is_err(),
            "{path} was accepted"
        );
    }
    assert_eq!(env.stored(), before);
    assert!(!env.dir.0.join("outside.txt").exists());
}

#[test]
fn symbolic_links_are_never_followed() {
    let mut env = Env::new();
    let outside = env.dir.0.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.txt"), b"secret").unwrap();
    let files = env.files();
    fs::create_dir_all(files.join("real")).unwrap();

    // A link as a parent directory, pointing outside and inside the project.
    symlink(&outside, files.join("escape")).unwrap();
    symlink(files.join("real"), files.join("inside")).unwrap();
    // A link as the file itself.
    symlink(outside.join("secret.txt"), files.join("secret.txt")).unwrap();

    let cases = [
        ("escape/new.txt", "files/escape"),
        ("escape/secret.txt", "files/escape"),
        ("inside/new.txt", "files/inside"),
        ("secret.txt", "files/secret.txt"),
    ];
    for (path, link) in cases {
        let artifact = env.add(path, "generated");
        let before = env.stored();
        assert_artifact_error(
            env.create(artifact, b"payload"),
            |e| matches!(e, ArtifactError::SymbolicLink { at, .. } if at == link),
        );
        assert_eq!(env.stored(), before, "{path}");
        match env.project().read_artifact(artifact) {
            Err(StoreError::Artifact(ArtifactError::SymbolicLink { at, .. })) => {
                assert_eq!(at, link)
            }
            other => panic!("{path}: {other:?}"),
        }
        assert!(
            matches!(
                env.status(artifact),
                ArtifactStatus::Unreadable(ArtifactError::SymbolicLink { .. })
            ),
            "{path}"
        );
    }
    // Nothing was written outside or through a link.
    let mut names: Vec<_> = fs::read_dir(&outside)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(names, ["secret.txt"]);
    assert_eq!(fs::read(outside.join("secret.txt")).unwrap(), b"secret");
    assert!(!files.join("real/new.txt").exists());
    assert!(env.staged().is_empty());
}

#[test]
fn a_linked_files_directory_is_refused() {
    let mut env = Env::new();
    let outside = env.dir.0.join("outside");
    fs::create_dir_all(&outside).unwrap();
    symlink(&outside, env.files()).unwrap();
    let artifact = env.add("a.txt", "generated");
    assert_artifact_error(
        env.create(artifact, b"payload"),
        |e| matches!(e, ArtifactError::SymbolicLink { at, .. } if at == "files"),
    );
    assert!(!outside.join("a.txt").exists());
}

#[test]
fn a_file_where_a_directory_is_needed_is_reported() {
    let mut env = Env::new();
    let file = env.add("docs", "generated");
    env.create(file, b"a file").unwrap();
    let nested = env.add("docs/inner.md", "generated");
    assert_artifact_error(
        env.create(nested, b"payload"),
        |e| matches!(e, ArtifactError::NotADirectory { at, .. } if at == "files/docs"),
    );
}

#[test]
fn paths_that_differ_only_in_case_are_refused() {
    let mut env = Env::new();
    let first = env.add("Readme.md", "generated");
    let second = env.add("README.md", "generated");
    // On a case-insensitive file system both name one file, so neither is
    // written.
    assert_artifact_error(
        env.create(first, b"first"),
        |e| matches!(e, ArtifactError::PathConflict { other, .. } if *other == second),
    );
    assert_artifact_error(
        env.create(second, b"second"),
        |e| matches!(e, ArtifactError::PathConflict { other, .. } if *other == first),
    );
    assert!(!env.files().join("Readme.md").exists());
    assert!(!env.files().join("README.md").exists());
}

#[test]
fn a_failed_write_leaves_no_partial_file_and_no_record() {
    let mut env = Env::new();
    let artifact = env.add("locked/out.bin", "generated");
    let locked = env.files().join("locked");
    fs::create_dir_all(&locked).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
    let before = env.stored();

    assert_artifact_error(env.create(artifact, &[7; 100_000]), |e| {
        matches!(e, ArtifactError::PermissionDenied { .. })
    });
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!locked.join("out.bin").exists());
    assert!(env.staged().is_empty(), "{:?}", env.staged());
    assert_eq!(env.stored(), before);
    assert_eq!(env.recorded(artifact), None);

    // After the cause is removed, the write succeeds.
    env.create(artifact, &[7; 100_000]).unwrap();
    assert_eq!(fs::read(locked.join("out.bin")).unwrap().len(), 100_000);
}

#[test]
fn a_failed_save_restores_the_file() {
    let mut env = Env::new();
    let created = env.add("new.txt", "generated");
    let replaced = env.add("old.txt", "generated");
    env.create(replaced, b"version 1").unwrap();
    let before = env.stored();
    let path = env.directory().join(DATABASE_FILE);
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER fail BEFORE UPDATE ON project_ir BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .unwrap();

    // A new file is removed again.
    assert!(matches!(
        env.create(created, b"new"),
        Err(StoreError::Sqlite(_))
    ));
    assert!(!env.files().join("new.txt").exists());

    // A replaced file gets its previous content back.
    let expected = env.recorded(replaced).unwrap();
    assert!(matches!(
        env.write(
            &generator(),
            replaced,
            b"version 2",
            WriteMode::Replace { expected }
        ),
        Err(StoreError::Sqlite(_))
    ));
    assert_eq!(fs::read(env.files().join("old.txt")).unwrap(), b"version 1");

    // Neither the database nor the open project changed.
    assert_eq!(env.stored(), before);
    assert!(env.staged().is_empty(), "{:?}", env.staged());
    assert_eq!(env.recorded(created), None);
    Connection::open(&path)
        .unwrap()
        .execute_batch("DROP TRIGGER fail;")
        .unwrap();
    env.reopen();
    assert!(matches!(env.status(created), ArtifactStatus::NotProduced));
    assert!(matches!(env.status(replaced), ArtifactStatus::Unchanged));
}

#[test]
fn external_changes_are_reported_and_never_overwritten() {
    let mut env = Env::new();
    let changed = env.add("changed.txt", "generated");
    let missing = env.add("missing.txt", "generated");
    let unchanged = env.add("unchanged.txt", "generated");
    let pending = env.add("pending.txt", "generated");
    for artifact in [changed, missing, unchanged] {
        env.create(artifact, b"original").unwrap();
    }
    fs::write(env.files().join("changed.txt"), b"edited outside").unwrap();
    fs::remove_file(env.files().join("missing.txt")).unwrap();
    env.reopen();
    let before = env.stored();

    match env.status(changed) {
        ArtifactStatus::Changed { digest, size_bytes } => {
            assert_eq!(
                (digest.as_str(), size_bytes),
                (sha256(b"edited outside").as_str(), 14)
            )
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(env.status(missing), ArtifactStatus::Missing));
    assert!(matches!(env.status(unchanged), ArtifactStatus::Unchanged));
    assert!(matches!(env.status(pending), ArtifactStatus::NotProduced));
    let file = env.project().read_artifact(changed).unwrap();
    assert_eq!(
        (file.bytes.as_slice(), file.matches_record),
        (&b"edited outside"[..], false)
    );
    // Checking and reading record nothing.
    assert_eq!(env.stored(), before);
    assert_eq!(env.recorded(changed).unwrap(), digest(b"original"));

    // A replacement does not overwrite the external change.
    let expected = env.recorded(changed).unwrap();
    assert_artifact_error(
        env.write(
            &generator(),
            changed,
            b"regenerated",
            WriteMode::Replace { expected },
        ),
        |e| {
            matches!(e, ArtifactError::ExternallyModified { path, found, .. }
                if path == "files/changed.txt" && *found == digest(b"edited outside"))
        },
    );
    assert_eq!(
        fs::read(env.files().join("changed.txt")).unwrap(),
        b"edited outside"
    );
    // A missing file is not silently recreated by a replacement.
    let expected = env.recorded(missing).unwrap();
    assert_artifact_error(
        env.write(&generator(), missing, b"x", WriteMode::Replace { expected }),
        |e| matches!(e, ArtifactError::Missing { .. }),
    );
    assert_eq!(env.stored(), before);
    assert!(env.staged().is_empty());
}

#[test]
fn unknown_artifacts_are_reported() {
    let mut env = Env::new();
    let unknown = ArtifactId::from_uuid(uuid::Uuid::from_u128(1));
    assert_artifact_error(
        env.create(unknown, b"x"),
        |e| matches!(e, ArtifactError::UnknownArtifact(id) if *id == unknown),
    );
    assert!(!env.files().exists());
}
