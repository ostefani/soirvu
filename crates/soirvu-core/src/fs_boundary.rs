// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna

//! Directory-relative filesystem operations for interchange and blobs.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path};

pub(crate) struct Directory {
    #[cfg(unix)]
    handle: File,
    #[cfg(windows)]
    guards: Vec<std::sync::Arc<File>>,
    #[cfg(not(unix))]
    path: std::path::PathBuf,
}

fn invalid_path() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "expected a path inside the selected directory",
    )
}

impl Directory {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            Ok(Self {
                handle: fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
                    .open(path)?,
            })
        }
        #[cfg(windows)]
        {
            // Canonicalize only the user-selected root. Each component below
            // that root is opened without following reparse points.
            let path = path.canonicalize()?;
            let guard = windows_directory(&path)?;
            Ok(Self {
                path,
                guards: vec![std::sync::Arc::new(guard)],
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            if !checked_metadata(path)?.is_dir() {
                return Err(invalid_path());
            }
            Ok(Self {
                path: path.to_path_buf(),
            })
        }
    }

    fn child(&self, name: &Path, create: bool) -> io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let name = c_name(name)?;
            if create {
                // SAFETY: the descriptor and NUL-terminated name are valid.
                let result =
                    unsafe { libc::mkdirat(self.handle.as_raw_fd(), name.as_ptr(), 0o700) };
                if result != 0 {
                    let error = io::Error::last_os_error();
                    if error.kind() != io::ErrorKind::AlreadyExists {
                        return Err(error);
                    }
                }
            }
            Ok(Self {
                handle: self.open_at(&name, libc::O_RDONLY | libc::O_DIRECTORY)?,
            })
        }
        #[cfg(not(unix))]
        {
            let path = self.path.join(name);
            if create {
                match fs::create_dir(&path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
            }
            #[cfg(windows)]
            {
                let mut guards = self.guards.clone();
                guards.push(std::sync::Arc::new(windows_directory(&path)?));
                Ok(Self { path, guards })
            }
            #[cfg(not(windows))]
            Self::open(&path)
        }
    }

    pub(crate) fn directory(&self, path: &Path, create: bool) -> io::Result<Self> {
        let mut dir = self.duplicate()?;
        for component in path.components() {
            let Component::Normal(name) = component else {
                return Err(invalid_path());
            };
            dir = dir.child(Path::new(name), create)?;
        }
        Ok(dir)
    }

    fn duplicate(&self) -> io::Result<Self> {
        #[cfg(unix)]
        return Ok(Self {
            handle: self.handle.try_clone()?,
        });
        #[cfg(not(unix))]
        return Ok(Self {
            path: self.path.clone(),
            #[cfg(windows)]
            guards: self.guards.clone(),
        });
    }

    pub(crate) fn open_regular(&self, relative: &Path) -> io::Result<File> {
        let (parent, name) = split(relative)?;
        let dir = self.directory(parent, false)?;
        #[cfg(unix)]
        let file = dir.open_at(&c_name(name)?, libc::O_RDONLY | libc::O_NONBLOCK)?;
        #[cfg(not(unix))]
        let file = {
            let path = dir.path.join(name);
            if !checked_metadata(&path)?.is_file() {
                return Err(invalid_path());
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                // Open the final reparse point itself for handle-based validation.
                fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(0x0020_0000)
                    .open(path)?
            }
            #[cfg(not(windows))]
            File::open(path)?
        };
        if !regular_metadata(&file.metadata()?) {
            return Err(invalid_path());
        }
        Ok(file)
    }

    /// Publish a complete file without replacing an existing name. A volume
    /// that cannot create hard links fails before publishing any bytes there.
    pub(crate) fn publish<T>(
        &self,
        relative: &Path,
        write: impl FnOnce(&mut File) -> crate::error::Result<T>,
    ) -> crate::error::Result<T> {
        self.stage(relative, write, false)
    }

    /// Replace an app-owned file with a complete, synced sibling.
    pub(crate) fn replace(&self, relative: &Path, bytes: &[u8]) -> crate::error::Result<()> {
        self.stage(relative, |file| {
            file.write_all(bytes)?;
            Ok(())
        }, true)
    }

    fn stage<T>(
        &self,
        relative: &Path,
        write: impl FnOnce(&mut File) -> crate::error::Result<T>,
        replace: bool,
    ) -> crate::error::Result<T> {
        let (parent, name) = split(relative)?;
        let dir = self.directory(parent, false)?;
        let temp = format!(".soirvu-{}.tmp", uuid::Uuid::now_v7());
        let temp = Path::new(&temp);
        #[cfg(unix)]
        let mut file = dir.open_at(
            &c_name(temp)?,
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
        )?;
        #[cfg(not(unix))]
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(dir.path.join(temp))?;
        let result = (|| {
            let output = write(&mut file)?;
            file.sync_all()?;
            drop(file);
            if replace {
                dir.rename(temp, name)?;
            } else {
                dir.link(temp, name)?;
            }
            #[cfg(unix)]
            if let Err(error) = dir.handle.sync_all() {
                if !replace {
                    let _ = dir.unlink(name);
                }
                return Err(error.into());
            }
            Ok(output)
        })();
        let _ = dir.unlink(temp);
        result
    }

    pub(crate) fn write(
        &self,
        relative: &Path,
        bytes: impl AsRef<[u8]>,
    ) -> crate::error::Result<()> {
        self.publish(relative, |file| {
            file.write_all(bytes.as_ref())?;
            Ok(())
        })
    }

    pub(crate) fn remove_file(&self, relative: &Path) -> io::Result<()> {
        let (parent, name) = split(relative)?;
        self.directory(parent, false)?.unlink(name)
    }

    #[cfg(unix)]
    fn open_at(&self, name: &std::ffi::CStr, flags: libc::c_int) -> io::Result<File> {
        use std::os::fd::{AsRawFd, FromRawFd};
        // SAFETY: the descriptor and NUL-terminated name are valid.
        let fd = unsafe {
            libc::openat(
                self.handle.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: openat returned a new owned descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let from = c_name(from)?;
            let to = c_name(to)?;
            let fd = self.handle.as_raw_fd();
            // SAFETY: both names and the directory descriptor are valid.
            if unsafe { libc::renameat(fd, from.as_ptr(), fd, to.as_ptr()) } != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
        #[cfg(not(unix))]
        crate::folder::rename_retry(&self.path.join(from), &self.path.join(to))
    }

    fn link(&self, from: &Path, to: &Path) -> io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let from = c_name(from)?;
            let to = c_name(to)?;
            let fd = self.handle.as_raw_fd();
            // SAFETY: both names and the directory descriptor are valid.
            if unsafe { libc::linkat(fd, from.as_ptr(), fd, to.as_ptr(), 0) } != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
        #[cfg(not(unix))]
        fs::hard_link(self.path.join(from), self.path.join(to))
    }

    fn unlink(&self, path: &Path) -> io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let path = c_name(path)?;
            // SAFETY: the descriptor and NUL-terminated name are valid.
            if unsafe { libc::unlinkat(self.handle.as_raw_fd(), path.as_ptr(), 0) } != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
        #[cfg(not(unix))]
        fs::remove_file(self.path.join(path))
    }
}

fn split(path: &Path) -> io::Result<(&Path, &Path)> {
    let Some(Component::Normal(name)) = path.components().next_back() else {
        return Err(invalid_path());
    };
    Ok((path.parent().ok_or_else(invalid_path)?, Path::new(name)))
}

#[cfg(unix)]
fn c_name(name: &Path) -> io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(name.as_os_str().as_bytes()).map_err(|_| invalid_path())
}

fn regular_metadata(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    metadata.is_file()
}

#[cfg(not(unix))]
fn checked_metadata(path: &Path) -> io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(invalid_path());
        }
    }
    if metadata.file_type().is_symlink() {
        return Err(invalid_path());
    }
    Ok(metadata)
}

#[cfg(windows)]
fn windows_directory(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_SHARE_READ_WRITE: u32 = 3;
    let file = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
        return Err(invalid_path());
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn replacing_metadata_does_not_write_through_a_destination_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("other");
        fs::write(&target, b"untouched").unwrap();
        let name = Path::new("header.json");
        std::os::unix::fs::symlink(&target, tmp.path().join(name)).unwrap();

        let root = Directory::open(tmp.path()).unwrap();
        root.replace(name, b"new header").unwrap();
        assert_eq!(fs::read(target).unwrap(), b"untouched");
        assert_eq!(fs::read(tmp.path().join(name)).unwrap(), b"new header");
    }

    #[test]
    fn publication_does_not_replace_existing_files_or_publish_failed_writes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Directory::open(tmp.path()).unwrap();
        let name = Path::new("export.json");
        root.write(name, b"first").unwrap();
        assert!(matches!(
            root.write(name, b"second"),
            Err(crate::error::Error::Io(error)) if error.kind() == io::ErrorKind::AlreadyExists
        ));
        assert_eq!(fs::read(tmp.path().join(name)).unwrap(), b"first");
        let aborted = Path::new("aborted");
        let failure = root.publish(aborted, |file| -> crate::error::Result<()> {
            file.write_all(b"partial")?;
            Err(io::Error::other("aborted").into())
        });
        assert!(failure.is_err());
        assert!(!tmp.path().join(aborted).exists());
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn child_paths_do_not_follow_links_and_root_survives_rename() {
        let tmp = tempfile::tempdir().unwrap();
        let original = tmp.path().join("root");
        let moved = tmp.path().join("moved");
        let other = tmp.path().join("other");
        fs::create_dir(&original).unwrap();
        fs::create_dir(&other).unwrap();
        let root = Directory::open(&original).unwrap();
        fs::rename(&original, &moved).unwrap();
        std::os::unix::fs::symlink(&other, &original).unwrap();
        std::os::unix::fs::symlink(&other, moved.join("child")).unwrap();

        assert!(root.directory(Path::new("child"), true).is_err());
        assert!(root.open_regular(Path::new("child/file")).is_err());
        root.write(Path::new("file"), b"bound to root").unwrap();
        assert_eq!(fs::read(moved.join("file")).unwrap(), b"bound to root");
        assert!(!other.join("file").exists());
    }
}
