use std::{
    collections::HashSet,
    ffi::OsString,
    path::{Component, Path, PathBuf},
    time::{Duration, SystemTime},
};

use clap::builder::OsStr;
use fuse_mt::{
    DirectoryEntry, FileAttr, FileType, FilesystemMT, RequestInfo, ResultOpen, ResultReaddir,
};
use libc::ENOENT;
use tracing::{debug, info};

use super::libc_wrappers::mode_to_filetype;

const TTL: Duration = Duration::from_secs(1);

trait ToFileAttr {
    fn to_file_attr(&self) -> FileAttr;
}

impl<T> ToFileAttr for Option<T> {
    fn to_file_attr(&self) -> FileAttr {
        FileAttr {
            size: 0,
            blocks: 0,
            atime: SystemTime::UNIX_EPOCH,
            mtime: SystemTime::UNIX_EPOCH,
            ctime: SystemTime::UNIX_EPOCH,
            crtime: SystemTime::UNIX_EPOCH,
            kind: FileType::Directory,
            perm: 0o0755,
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
            flags: 0,
        }
    }
}

impl ToFileAttr for libc::stat {
    fn to_file_attr(&self) -> FileAttr {
        // st_mode encodes both the kind and the permissions
        let kind = mode_to_filetype(self.st_mode);
        let perm = (self.st_mode & 0o7777) as u16;

        FileAttr {
            size: self.st_size as u64,
            blocks: self.st_blocks as u64,
            atime: SystemTime::UNIX_EPOCH
                + Duration::from_secs(self.st_atime as u64)
                + Duration::from_nanos(self.st_atime_nsec as u64),
            mtime: SystemTime::UNIX_EPOCH
                + Duration::from_secs(self.st_mtime as u64)
                + Duration::from_nanos(self.st_mtime_nsec as u64),
            ctime: SystemTime::UNIX_EPOCH
                + Duration::from_secs(self.st_ctime as u64)
                + Duration::from_nanos(self.st_ctime_nsec as u64),
            crtime: SystemTime::UNIX_EPOCH,
            kind,
            perm,
            nlink: self.st_nlink as u32,
            uid: self.st_uid,
            gid: self.st_gid,
            rdev: self.st_rdev as u32,
            flags: 0,
        }
    }
}

#[derive(Debug)]
struct Entry {
    source: PathBuf,
}

#[derive(Debug)]
pub struct TagFS {
    files: Vec<Entry>,
    tags: HashSet<OsString>,
}

impl<'a> TagFS {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            tags: HashSet::new()
        }
    }

    pub fn add_file(&mut self, source: &'a Path, tags: HashSet<OsString>) {
        info!(file = ?source, ?tags, "add_file");
        self.files.push(Entry {
            source: source.to_path_buf(),
        });
        for tag in tags {
            self.tags.insert(tag);
        }
    }
}

impl FilesystemMT for TagFS {
    fn getattr(
        &self,
        _req: fuse_mt::RequestInfo,
        path: &std::path::Path,
        fh: Option<u64>,
    ) -> fuse_mt::ResultEntry {
        info!(path = debug(path), fh = debug(fh), "getattr");

        if let Some(fh) = fh {
            match super::libc_wrappers::fstat(fh) {
                Ok(stat) => Ok((TTL, stat.to_file_attr())),
                Err(e) => Err(e.raw_os_error().unwrap_or(libc::ENOENT)),
            }
        } else if path.components().all(|c| match c {
            Component::Prefix(prefix_component) => todo!(),
            Component::RootDir => true,
            Component::CurDir => false,
            Component::ParentDir => false,
            Component::Normal(tag) => self.tags.contains(tag),
        }) {
            debug!(?path, "tag dir");
            Ok((TTL, fh.to_file_attr()))
        } else {
            for component in path.components() {
                info!(?path, ?component);
            }
            info!(path = debug(path), "TODO: lookup");
            Err(ENOENT)
        }
    }
    fn opendir(&self, _req: RequestInfo, path: &Path, flags: u32) -> ResultOpen {
        info!(
            path = debug(path),
            flags = format!("{:#o}", flags),
            "opendir"
        );
        if path.components().all(|c| match c {
            Component::Prefix(prefix_component) => todo!(),
            Component::RootDir => true,
            Component::CurDir => false,
            Component::ParentDir => false,
            Component::Normal(tag) => self.tags.contains(tag),
        }) {
            Ok((0, 0))
        } else {
            info!(path = debug(path), "TODO: lookup");
            Err(ENOENT)
        }
    }

    fn readdir(&self, _req: RequestInfo, path: &Path, fh: u64) -> ResultReaddir {
        info!(path = debug(path), fh = debug(fh), "readdir");
        let tags = path.components().filter_map(|c| match c {
            Component::Normal(p) => Some(p.to_os_string()),
            _ => None,
        }).collect::<HashSet<_>>();
        info!(?tags, ?path, "lookup");
        let mut entries = vec![
                DirectoryEntry {
                    name: ".".into(),
                    kind: FileType::Directory,
                },
                DirectoryEntry {
                    name: "..".into(),
                    kind: FileType::Directory,
                },
            ];
            for e in &self.files {
                let name = e.source.file_name().unwrap().into();
                info!(?name, ?path, "readdir");
                entries.push(DirectoryEntry {
                    name,
                    kind: FileType::RegularFile,
                });
            }
            for tag in &self.tags {
                if !tags.contains(tag) {
                entries.push(DirectoryEntry { name: tag.into(), kind: FileType::Directory });
                }
            }
            
        Ok(entries)
    }
}