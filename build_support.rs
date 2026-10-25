use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
enum SourceEntry {
    File,
    Directory,
}

// Copy packaged sources incrementally and remove only previously copied paths.
// The sibling manifest distinguishes maintained input from CMake-generated files.
pub(crate) fn copy_tree(source: &Path, destination: &Path) {
    let mut current = BTreeMap::new();
    collect_entries(source, source, &mut current);
    let manifest = destination.with_extension("source-manifest");
    let previous = read_manifest(&manifest);
    let mut obsolete: Vec<_> = previous
        .iter()
        .filter(|(path, kind)| current.get(*path) != Some(*kind))
        .collect();
    // Remove children before parents, including file/directory type changes.
    obsolete.sort_by_key(|(path, _)| std::cmp::Reverse(path.components().count()));
    for (relative, kind) in obsolete {
        let target = destination.join(relative);
        let result = match kind {
            SourceEntry::File => std::fs::remove_file(&target),
            SourceEntry::Directory => std::fs::remove_dir(&target),
        };
        match result {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            // Preserve directories containing generated/unmanaged files.
            Err(error)
                if *kind == SourceEntry::Directory
                    && error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
            Err(error) => panic!(
                "remove obsolete native source {}: {error}",
                target.display()
            ),
        }
    }

    std::fs::create_dir_all(destination).expect("create native source directory");
    for (relative, kind) in &current {
        let target = destination.join(relative);
        match kind {
            SourceEntry::Directory => {
                std::fs::create_dir_all(&target).expect("create native source subdirectory");
            }
            SourceEntry::File => {
                let input = source.join(relative);
                let content = std::fs::read(&input).expect("read native source file");
                if std::fs::read(&target).ok().as_deref() != Some(content.as_slice()) {
                    std::fs::copy(input, target).expect("copy native source");
                }
            }
        }
    }
    let mut contents = Vec::new();
    for (path, kind) in current {
        contents.push(match kind {
            SourceEntry::File => b'F',
            SourceEntry::Directory => b'D',
        });
        // Cargo package source paths are UTF-8; NUL cannot occur in a filename.
        contents.extend_from_slice(
            path.to_str()
                .expect("UTF-8 packaged source path")
                .as_bytes(),
        );
        contents.push(0);
    }
    if std::fs::read(&manifest).ok().as_deref() != Some(contents.as_slice()) {
        let temporary = manifest.with_extension("source-manifest.tmp");
        std::fs::write(&temporary, contents).expect("write native source manifest");
        std::fs::rename(temporary, manifest).expect("replace native source manifest");
    }
}

fn collect_entries(root: &Path, directory: &Path, entries: &mut BTreeMap<PathBuf, SourceEntry>) {
    for entry in std::fs::read_dir(directory).expect("read packaged native source") {
        let entry = entry.expect("native source entry");
        let kind = entry.file_type().expect("native source type");
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .expect("native source relative path")
            .to_owned();
        if kind.is_dir() {
            entries.insert(relative, SourceEntry::Directory);
            collect_entries(root, &path, entries);
        } else {
            assert!(
                kind.is_file(),
                "native source must contain only regular files and directories"
            );
            entries.insert(relative, SourceEntry::File);
        }
    }
}

fn read_manifest(path: &Path) -> BTreeMap<PathBuf, SourceEntry> {
    let data = match std::fs::read(path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return BTreeMap::new(),
        Err(error) => panic!("read native source manifest: {error}"),
    };
    data.split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .map(|record| {
            let kind = match record[0] {
                b'F' => SourceEntry::File,
                b'D' => SourceEntry::Directory,
                _ => panic!("invalid native source manifest entry"),
            };
            let relative =
                PathBuf::from(std::str::from_utf8(&record[1..]).expect("UTF-8 source manifest"));
            assert!(
                !relative.as_os_str().is_empty()
                    && relative
                        .components()
                        .all(|c| matches!(c, Component::Normal(_))),
                "native source manifest paths must stay within the destination"
            );
            (relative, kind)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::copy_tree;
    use std::fs::{self, File, FileTimes};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, UNIX_EPOCH};

    struct Sandbox(std::path::PathBuf);

    impl Sandbox {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            loop {
                let path = std::env::temp_dir().join(format!(
                    "libntirpc-copy-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("create copy test sandbox: {error}"),
                }
            }
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn native_copy_preserves_unchanged_timestamps_and_refreshes_content() {
        let sandbox = Sandbox::new();
        let source = sandbox.0.join("source");
        let destination = sandbox.0.join("destination");
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::write(source.join("nested/header.h"), "old content").unwrap();
        copy_tree(&source, &destination);
        let copied = destination.join("nested/header.h");
        let timestamp = UNIX_EPOCH + Duration::from_secs(1);
        File::open(&copied)
            .unwrap()
            .set_times(FileTimes::new().set_modified(timestamp))
            .unwrap();
        copy_tree(&source, &destination);
        assert_eq!(
            fs::metadata(&copied).unwrap().modified().unwrap(),
            timestamp
        );
        assert_eq!(fs::read_to_string(&copied).unwrap(), "old content");

        // Equal-sized changes still have to propagate.
        fs::write(source.join("nested/header.h"), "new content").unwrap();
        fs::write(source.join("new.c"), "added file").unwrap();
        copy_tree(&source, &destination);
        assert_eq!(fs::read_to_string(copied).unwrap(), "new content");
        assert_eq!(
            fs::read_to_string(destination.join("new.c")).unwrap(),
            "added file"
        );
    }

    #[test]
    fn native_copy_removes_deleted_sources_and_preserves_generated_outputs() {
        let sandbox = Sandbox::new();
        let source = sandbox.0.join("source");
        let destination = sandbox.0.join("destination");
        fs::create_dir_all(source.join("removed/nested")).unwrap();
        fs::create_dir_all(source.join("with-generated")).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("generated-before-first-copy"), "keep").unwrap();
        fs::write(source.join("removed/nested/header.h"), "obsolete").unwrap();
        fs::write(source.join("with-generated/source.c"), "obsolete").unwrap();
        copy_tree(&source, &destination);
        fs::write(destination.join("with-generated/output.spec"), "generated").unwrap();
        fs::remove_dir_all(source.join("removed")).unwrap();
        fs::remove_dir_all(source.join("with-generated")).unwrap();
        copy_tree(&source, &destination);
        assert!(!destination.join("removed").exists());
        assert!(!destination.join("with-generated/source.c").exists());
        assert_eq!(
            fs::read_to_string(destination.join("with-generated/output.spec")).unwrap(),
            "generated"
        );
        assert_eq!(
            fs::read_to_string(destination.join("generated-before-first-copy")).unwrap(),
            "keep"
        );
        copy_tree(&source, &destination); // Reconciliation is idempotent.
        assert!(destination.join("with-generated/output.spec").is_file());
    }

    #[test]
    fn native_copy_handles_file_directory_transitions_and_special_path_characters() {
        let sandbox = Sandbox::new();
        let source = sandbox.0.join("source");
        let destination = sandbox.0.join("destination");
        fs::create_dir_all(source.join("directory-to-file")).unwrap();
        fs::write(source.join("directory-to-file/header.h"), "old").unwrap();
        fs::write(source.join("file-to-directory"), "old").unwrap();
        let unusual = "space and\nnewline.h";
        fs::write(source.join(unusual), "old").unwrap();
        copy_tree(&source, &destination);
        fs::remove_dir_all(source.join("directory-to-file")).unwrap();
        fs::write(source.join("directory-to-file"), "new file").unwrap();
        fs::remove_file(source.join("file-to-directory")).unwrap();
        fs::create_dir(source.join("file-to-directory")).unwrap();
        fs::write(source.join("file-to-directory/header.h"), "new header").unwrap();
        fs::remove_file(source.join(unusual)).unwrap();
        copy_tree(&source, &destination);
        assert_eq!(
            fs::read_to_string(destination.join("directory-to-file")).unwrap(),
            "new file"
        );
        assert_eq!(
            fs::read_to_string(destination.join("file-to-directory/header.h")).unwrap(),
            "new header"
        );
        assert!(!destination.join(unusual).exists());
    }
}
