//! Unix descriptor-relative file access. The directory lock covers read, edit and rename.
use crate::Error;
use rustix::fs::{self, AtFlags, Mode, OFlags};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Component, Path},
    sync::atomic::{AtomicU64, Ordering},
};
use tasks_core::{Error as CoreError, messages::MessageCode as C};

fn io(e: impl std::error::Error + Send + Sync + 'static) -> Error {
    CoreError::caused(C::Io, e).into()
}
fn fail(c: C) -> Error {
    CoreError::new(c, serde_json::json!({})).into()
}
fn link_error(e: rustix::io::Errno) -> Error {
    if e == rustix::io::Errno::LOOP {
        fail(C::Symlink)
    } else {
        io(e)
    }
}
fn is_link(stat: &fs::Stat) -> bool {
    fs::FileType::from_raw_mode(stat.st_mode) == fs::FileType::Symlink
}
fn directory_step(dir: &File, name: &std::ffi::OsStr) -> Result<File, Error> {
    let stat = fs::statat(dir, name, AtFlags::SYMLINK_NOFOLLOW).map_err(io)?;
    if is_link(&stat) {
        return Err(fail(C::Symlink));
    }
    Ok(File::from(
        fs::openat(
            dir,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(link_error)?,
    ))
}
fn base_directory(base: &Path) -> Result<File, Error> {
    let start = if base.is_absolute() { "/" } else { "." };
    let mut dir = File::from(
        fs::open(
            start,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(link_error)?,
    );
    for c in base.components() {
        match c {
            Component::Normal(name) => dir = directory_step(&dir, name)?,
            Component::CurDir | Component::RootDir => (),
            _ => return Err(fail(C::Path)),
        }
    }
    Ok(dir)
}

pub(crate) struct Target {
    pub dir: File,
    pub name: std::ffi::OsString,
}
impl Target {
    pub fn open(base: &Path, path: &Path) -> Result<Self, Error> {
        if path.as_os_str().is_empty()
            || path.is_absolute()
            || path
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(fail(C::Path));
        }
        let mut parts = path.iter().peekable();
        let mut dir = base_directory(base)?;
        while let Some(name) = parts.next() {
            if parts.peek().is_none() {
                let target = Self {
                    dir,
                    name: name.to_os_string(),
                };
                #[cfg(test)]
                test_hooks::fire(&test_hooks::TARGET_OPENED);
                return Ok(target);
            }
            dir = directory_step(&dir, name)?;
        }
        Err(fail(C::Path))
    }
    /// Lock the stable directory inode, not the file replaced by rename.
    pub fn lock(&self) -> Result<(), Error> {
        fs::flock(&self.dir, fs::FlockOperation::LockExclusive).map_err(io)
    }
    fn stat(&self) -> Result<Option<fs::Stat>, Error> {
        match fs::statat(&self.dir, &self.name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => {
                if is_link(&stat) {
                    return Err(fail(C::Symlink));
                }
                if fs::FileType::from_raw_mode(stat.st_mode) != fs::FileType::RegularFile {
                    return Err(fail(C::FileType));
                }
                Ok(Some(stat))
            }
            Err(e) if e == rustix::io::Errno::NOENT => Ok(None),
            Err(e) => Err(io(e)),
        }
    }
    pub fn read(&self, limit: u64) -> Result<String, Error> {
        let stat = self
            .stat()?
            .ok_or_else(|| io(std::io::Error::from(std::io::ErrorKind::NotFound)))?;
        if stat.st_size < 0 || stat.st_size as u64 > limit {
            return Err(size(limit));
        }
        #[cfg(test)]
        test_hooks::fire(&test_hooks::BEFORE_READ_OPEN);
        let file = File::from(
            fs::openat(
                &self.dir,
                &self.name,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(link_error)?,
        );
        #[cfg(test)]
        test_hooks::fire(&test_hooks::READ_OPENED);
        let meta = file.metadata().map_err(io)?;
        if !meta.is_file() {
            return Err(fail(C::FileType));
        }
        if meta.len() > limit {
            return Err(size(limit));
        }
        let mut bytes = Vec::new();
        file.take(limit.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(io)?;
        if bytes.len() as u64 > limit {
            return Err(size(limit));
        }
        String::from_utf8(bytes).map_err(|e| CoreError::caused(C::Utf8, e).into())
    }
    pub fn write(&self, bytes: &[u8]) -> Result<(), Error> {
        self.stat()?;
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let mut temporary = None;
        for _ in 0..100 {
            let name = format!(
                ".tasks-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            match fs::openat(
                &self.dir,
                &name,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            ) {
                Ok(fd) => {
                    temporary = Some((name, File::from(fd)));
                    break;
                }
                Err(e) if e == rustix::io::Errno::EXIST => continue,
                Err(e) => return Err(io(e)),
            }
        }
        let (name, mut file) =
            temporary.ok_or_else(|| io(std::io::Error::from(std::io::ErrorKind::AlreadyExists)))?;
        let result = (|| {
            file.write_all(bytes).map_err(io)?;
            file.sync_all().map_err(io)?;
            #[cfg(test)]
            test_hooks::fire(&test_hooks::BEFORE_RENAME);
            self.stat()?;
            fs::renameat(&self.dir, &name, &self.dir, &self.name).map_err(io)
        })();
        if result.is_err() {
            // Keep the original error if cleanup also fails.
            let _ = fs::unlinkat(&self.dir, &name, AtFlags::empty());
        }
        result
    }
}
fn size(limit: u64) -> Error {
    CoreError::new(C::Size, serde_json::json!({"limit":limit})).into()
}

// Test-only scheduling points let the full tool pipeline exercise path races without sleeps.
#[cfg(test)]
mod test_hooks {
    use std::cell::RefCell;
    pub type Hook = RefCell<Option<Box<dyn FnOnce()>>>;
    thread_local! {
        pub static TARGET_OPENED: Hook = const { RefCell::new(None) };
        pub static BEFORE_READ_OPEN: Hook = const { RefCell::new(None) };
        pub static READ_OPENED: Hook = const { RefCell::new(None) };
        pub static BEFORE_RENAME: Hook = const { RefCell::new(None) };
    }
    pub fn fire(key: &'static std::thread::LocalKey<Hook>) {
        let callback = key.with(|hook| hook.borrow_mut().take());
        if let Some(callback) = callback {
            callback();
        }
    }
}

#[cfg(test)]
#[path = "files_tests.rs"]
mod pipeline_tests;

#[cfg(test)]
mod tests {
    use super::*;
    type TestResult = Result<(), Box<dyn std::error::Error>>;
    #[test]
    fn directory_descriptor_survives_path_replacement() -> TestResult {
        let root = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        std::fs::create_dir(root.path().join("folder"))?;
        std::fs::write(root.path().join("folder/list.json"), b"old")?;
        std::fs::write(outside.path().join("list.json"), b"outside")?;
        let target = Target::open(root.path(), Path::new("folder/list.json"))?;
        target.lock()?;
        std::fs::rename(root.path().join("folder"), root.path().join("pinned"))?;
        std::os::unix::fs::symlink(outside.path(), root.path().join("folder"))?;
        assert_eq!(target.read(100)?, "old");
        target.write(b"new")?;
        assert_eq!(std::fs::read(root.path().join("pinned/list.json"))?, b"new");
        assert_eq!(std::fs::read(outside.path().join("list.json"))?, b"outside");
        assert_eq!(std::fs::read_dir(root.path().join("pinned"))?.count(), 1);
        Ok(())
    }
}
