// Preserve native source timestamps so unchanged C files remain incremental.
pub(crate) fn copy_tree(source: &std::path::Path, destination: &std::path::Path) {
    std::fs::create_dir_all(destination).expect("create native source directory");
    for entry in std::fs::read_dir(source).expect("read packaged native source") {
        let entry = entry.expect("native source entry");
        let target = destination.join(entry.file_name());
        let file_type = entry.file_type().expect("native source type");
        if file_type.is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            assert!(
                file_type.is_file(),
                "native source must contain only regular files and directories"
            );
            let content = std::fs::read(entry.path()).expect("read native source file");
            if std::fs::read(&target).ok().as_deref() != Some(content.as_slice()) {
                std::fs::copy(entry.path(), target).expect("copy native source");
            }
        }
    }
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
}
