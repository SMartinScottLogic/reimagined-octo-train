use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    path::{Component, Path, PathBuf},
    time::{Duration, SystemTime},
};

use fuse_mt::{
    DirectoryEntry, FileAttr, FileType, FilesystemMT, RequestInfo, ResultOpen, ResultReaddir,
};
use itertools::Itertools as _;
use libc::ENOENT;
use tracing::{debug, info, instrument};

use crate::tagger::Tag;

use super::libc_wrappers::{mode_to_filetype, LibcWrapper, LibcWrapperReal};

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
pub struct TagFS<T> {
    files: Vec<Entry>,
    //tags: HashSet<OsString>,
    tags: HashMap<Tag, HashSet<usize>>,
    libc_wrapper: T, //Box<dyn LibcWrapper + Send + Sync>,
}

pub fn new() -> TagFS<LibcWrapperReal> {
    TagFS::<LibcWrapperReal>::new()
}

impl<'a, T> TagFS<T>
where
    T: LibcWrapper,
{
    fn new() -> Self {
        let libc_wrapper = T::new();
        Self {
            files: Vec::new(),
            tags: HashMap::new(),
            libc_wrapper,
        }
    }

    pub fn add_file(&mut self, source: &'a Path, tags: HashSet<Tag>) {
        info!(file = ?source, ?tags, "add_file");
        self.files.push(Entry {
            source: source.to_path_buf(),
        });
        let file_id = self.files.len() - 1;
        for tag in tags {
            self.tags.entry(tag).or_default().insert(file_id);
        }
    }

    fn contains_tag(&self, tag: &OsStr) -> bool {
        self.get_tag(tag).is_some()
    }

    fn get_tag(&self, tag: &OsStr) -> Option<(&Tag, &HashSet<usize>)> {
        self.tags.iter().find(|(t, _file_ids)| t.as_os_str() == tag)
    }
}

impl<T> FilesystemMT for TagFS<T>
where
    T: LibcWrapper,
{
    fn getattr(
        &self,
        _req: fuse_mt::RequestInfo,
        path: &std::path::Path,
        fh: Option<u64>,
    ) -> fuse_mt::ResultEntry {
        info!(path = debug(path), fh = debug(fh), "getattr");

        if let Some(fh) = fh {
            match self.libc_wrapper.fstat(fh) {
                Ok(stat) => Ok((TTL, stat.to_file_attr())),
                Err(e) => Err(e.raw_os_error().unwrap_or(libc::ENOENT)),
            }
        } else {
            match self.lookup(path) {
                LookupResult::Directory => Ok((TTL, fh.to_file_attr())),
                LookupResult::Missing => Err(ENOENT),
                LookupResult::File(source) => match self.libc_wrapper.lstat(source) {
                    Ok(stat) => Ok((TTL, stat.to_file_attr())),
                    Err(e) => Err(e.raw_os_error().unwrap_or(libc::ENOENT)),
                },
            }
        }
    }

    fn opendir(&self, _req: RequestInfo, path: &Path, flags: u32) -> ResultOpen {
        info!(
            path = debug(path),
            flags = format!("{:#o}", flags),
            "opendir"
        );
        if path.components().all(|c| match c {
            Component::Prefix(_prefix_component) => todo!(),
            Component::RootDir => true,
            Component::CurDir => false,
            Component::ParentDir => false,
            Component::Normal(tag) => self.contains_tag(tag),
        }) {
            Ok((0, 0))
        } else {
            info!(path = debug(path), "TODO: lookup");
            Err(ENOENT)
        }
    }

    fn readdir(&self, _req: RequestInfo, path: &Path, fh: u64) -> ResultReaddir {
        info!(path = debug(path), fh = debug(fh), "readdir");
        let tags = path
            .components()
            .filter_map(|c| match c {
                Component::Normal(p) => Some(p.to_os_string()),
                _ => None,
            })
            .collect::<HashSet<_>>();
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

        for (child_type, child_name) in get_children(path, &self.tags, &self.files) {
            info!(?child_type, name = ?child_name, "children");
            entries.push(DirectoryEntry {
                name: child_name.into(),
                kind: child_type,
            });
        }

        Ok(entries)
    }

    fn open(&self, _req: RequestInfo, path: &Path, flags: u32) -> ResultOpen {
        info!(?path, flags = format!("{:o}", flags), "open");

        match self.lookup(path) {
            LookupResult::Directory => Err(ENOENT),
            LookupResult::File(source) => self
                .libc_wrapper
                .open(&PathBuf::from(source), flags as i32)
                .map(|fh| (fh as u64, flags))
                .map_err(|e| e.raw_os_error().unwrap_or(ENOENT)),
            LookupResult::Missing => Err(ENOENT),
        }
    }

    fn release(
        &self,
        _req: RequestInfo,
        path: &Path,
        fh: u64,
        flags: u32,
        lock_owner: u64,
        flush: bool,
    ) -> fuse_mt::ResultEmpty {
        info!(
            ?path,
            fh,
            flags = format!("{:o}", flags),
            lock_owner,
            flush,
            "release"
        );
        self.libc_wrapper
            .close(fh as i32)
            .map_err(|e| e.raw_os_error().unwrap_or(ENOENT))
    }

    fn read(
        &self,
        _req: RequestInfo,
        path: &Path,
        fh: u64,
        offset: u64,
        size: u32,
        callback: impl FnOnce(fuse_mt::ResultSlice<'_>) -> fuse_mt::CallbackResult,
    ) -> fuse_mt::CallbackResult {
        info!(?path, fh, offset, size, "read");

        match self.libc_wrapper.read(fh as i32, offset as i64, size) {
            Ok(content) => callback(Ok(content.as_slice())),
            Err(e) => callback(Err(e.raw_os_error().unwrap_or(ENOENT))),
        }
    }

    fn unlink(&self, _req: RequestInfo, parent: &Path, name: &OsStr) -> fuse_mt::ResultEmpty {
        let path: PathBuf = parent.join(name);
        info!(?parent, ?name, ?path, "unlink");
        match self.lookup(&path) {
            LookupResult::Directory | LookupResult::Missing => Err(ENOENT),
            LookupResult::File(source) => match self.libc_wrapper.unlink(source) {
                Ok(_) => Ok(()),
                Err(e) => Err(e.raw_os_error().unwrap_or(ENOENT)),
            },
        }
    }
}

#[derive(Debug)]
enum LookupResult<'a> {
    Directory,
    File(&'a Path),
    Missing,
}
impl<'a, T> TagFS<T>
where
    T: LibcWrapper,
{
    #[instrument(skip(self))]
    fn lookup(&'a self, path: &Path) -> LookupResult<'a> {
        use LookupResult::*;
        info!(?path, "lookup");

        if path.components().all(|c| match c {
            Component::Prefix(_prefix_component) => todo!(),
            Component::RootDir => true,
            Component::CurDir => false,
            Component::ParentDir => false,
            Component::Normal(tag) => self.contains_tag(tag),
        }) {
            debug!(?path, "tag dir");
            Directory
        } else {
            let mut valid_files = None;
            let empty = PathBuf::new();
            let empty = empty.as_path();

            for component in path.parent().unwrap_or(empty).components() {
                info!(?path, ?component);
                if let Component::Normal(tag) = component {
                    if let Some(files) = self.get_tag(tag) {
                        let files = files.1;
                        if valid_files.is_none() {
                            valid_files = Some(files.clone());
                        } else {
                            valid_files =
                                Some(valid_files.unwrap().intersection(files).cloned().collect());
                        }
                        info!(?tag, ?valid_files, "found");
                    } else {
                        info!(?component, "missing");
                    }
                }
            }
            if let Some(files) = valid_files {
                let entry = files
                    .iter()
                    .flat_map(|idx| self.files.get(*idx))
                    .filter(|entry| entry.source.file_name() == path.file_name())
                    .take(1)
                    .next();
                match entry {
                    None => Missing,
                    Some(e) => File(e.source.as_path()),
                }
            } else {
                info!(path = debug(path), "failed lookup");
                Missing
            }
        }
    }
}

#[instrument(skip_all)]
fn get_children<'a, 'b, 'c>(
    root: &Path,
    tags: &'a HashMap<Tag, HashSet<usize>>,
    files: &'b [Entry],
) -> impl Iterator<Item = (FileType, &'c OsStr)>
where
    'a: 'c,
    'b: 'c,
{
    // TODO Filter out intrinsic tags NOT represented by residual files
    let root_tags = root
        .components()
        .filter_map(|c| match c {
            Component::Normal(p) => Some(p.to_os_string()),
            _ => None,
        })
        .collect::<HashSet<_>>();

    // Collect ids of files with ALL tags in path
    let file_ids = if root_tags.is_empty() {
        HashSet::new()
    } else {
        tags.iter()
            .filter(|(tag, _file_ids)| root_tags.contains(tag.as_os_str()))
            .map(|(_tag, file_ids)| file_ids)
            .fold(None, |acc, v| match acc {
                None => Some(v.clone()),
                Some(a) => Some(a.intersection(v).cloned().collect()),
            })
            .unwrap_or_default()
    };

    debug!(?file_ids, ?root_tags, ?root, "residue");

    // TODO Deal with name collision between 2x tags NOT in root, one intrinsic, one extrinsic

    let mut singleton_labels = HashSet::new();
    for tag in tags.keys() {
        debug!(?tag, ?root_tags, "detect singletons");
        if root_tags.contains(tag.as_os_str()) && tag.is_singleton() {
            singleton_labels.insert(tag.label());
        }
    }

    tags.iter()
        // Filter out tags already in path
        .filter(move |(t, _)| {
            debug!(?t, ?root_tags, "visited filter tag");
            !root_tags.contains(t.as_os_str())
        })
        // Filter out already seen filter tags
        .filter(move |(t, _)| {
            debug!(?t, ?singleton_labels, "singleton filter tag");
            !t.is_singleton() || !singleton_labels.contains(t.label())
        })
        // Remaining tags become directory entries
        .map(|(t, _)| (FileType::Directory, t.as_os_str()))
        .chain(
            // File ids become Regular File entries
            file_ids
                .into_iter()
                .filter_map(|file_id| files.get(file_id))
                .filter_map(|file| file.source.file_name())
                .unique()
                .map(|file_name| (FileType::RegularFile, file_name)),
        )
}

#[cfg(test)]
mod test {
    use std::{
        collections::{HashMap, HashSet},
        ffi::OsString,
        path::PathBuf,
        sync::Mutex,
    };

    use fuse_mt::{FilesystemMT as _, RequestInfo};
    use libc::{ENOENT, EPERM};
    use tracing_test::traced_test;

    use crate::{
        filesystem::{
            libc_wrappers::MockLibcWrapper,
            tagfs::{get_children, TagFS},
        },
        tagger::{Tag, TAG_SEPARATOR},
    };

    use super::Entry;

    // Mutex to ensure only one test at a time is accessing the global context for construction
    static MTX: Mutex<()> = Mutex::new(());

    #[traced_test]
    #[test]
    fn get_children_root() {
        let mut tags = HashMap::new();
        tags.insert(Tag::from("tag1"), HashSet::new());
        tags.insert(Tag::from("tag2"), HashSet::new());
        tags.insert(Tag::from("tag3"), HashSet::new());
        let files = vec![Entry {
            source: PathBuf::from("/fake/dir/where/file/exists/file1.txt"),
        }];

        let children = get_children(&PathBuf::from("/"), &tags, &files);
        assert_eq!(3, children.count());
    }

    #[traced_test]
    #[test]
    fn get_children_tag2() {
        let mut tags = HashMap::new();
        tags.insert(Tag::from("tag1"), HashSet::new());
        tags.insert(Tag::from("tag2"), HashSet::new());
        tags.insert(Tag::from("tag3"), HashSet::new());
        let files = vec![Entry {
            source: PathBuf::from("/fake/dir/where/file/exists/file1.txt"),
        }];

        let children = get_children(&PathBuf::from("/tag2"), &tags, &files);
        assert_eq!(2, children.count());
    }

    #[traced_test]
    #[test]
    fn get_children_tag2_tag1() {
        let mut tags = HashMap::new();
        tags.insert(Tag::from("tag1"), HashSet::new());
        tags.insert(Tag::from("tag2"), HashSet::new());
        tags.insert(Tag::from("tag3"), HashSet::new());
        let files = vec![Entry {
            source: PathBuf::from("/fake/dir/where/file/exists/file1.txt"),
        }];

        let children = get_children(&PathBuf::from("/tag2/tag1"), &tags, &files);
        assert_eq!(1, children.count());
    }

    #[traced_test]
    #[test]
    fn get_children_singleton_root() {
        let mut tags = HashMap::new();
        tags.insert(Tag::new("singleton", true, "v1"), HashSet::new());
        tags.insert(Tag::new("singleton", true, "v2"), HashSet::new());
        tags.insert(Tag::from("tag1"), HashSet::new());
        let files = vec![Entry {
            source: PathBuf::from("/fake/dir/where/file/exists/file1.txt"),
        }];

        let children = get_children(&PathBuf::from("/"), &tags, &files).collect::<HashSet<_>>();
        // Root shows all tags, no files
        assert_eq!(3, children.len());
        assert!(children.contains(&(
            fuse_mt::FileType::Directory,
            &OsString::from("singleton".to_owned() + TAG_SEPARATOR + "v1")
        )));
        assert!(children.contains(&(
            fuse_mt::FileType::Directory,
            &OsString::from("singleton".to_owned() + TAG_SEPARATOR + "v2")
        )));
        assert!(children.contains(&(fuse_mt::FileType::Directory, &OsString::from("tag1"))));
    }

    #[traced_test]
    #[test]
    fn get_children_singleton_child_file() {
        let mut tags = HashMap::new();
        tags.insert(Tag::new("singleton", true, "v1"), HashSet::from([0]));
        tags.insert(Tag::new("singleton", true, "v2"), HashSet::new());
        tags.insert(Tag::from("tag1"), HashSet::new());
        let files = vec![Entry {
            source: PathBuf::from("/fake/dir/where/file/exists/file1.txt"),
        }];

        let children = get_children(
            &PathBuf::from("/singleton".to_owned() + TAG_SEPARATOR + "v1"),
            &tags,
            &files,
        )
        .collect::<HashSet<_>>();
        // Inner shows only non-singleton collisions, and related files
        assert_eq!(2, children.len());
        assert!(children.contains(&(fuse_mt::FileType::Directory, &OsString::from("tag1"))));
        assert!(children.contains(&(fuse_mt::FileType::RegularFile, &OsString::from("file1.txt"))));
    }

    #[traced_test]
    #[test]
    fn get_children_singleton_child_nofile() {
        let mut tags = HashMap::new();
        tags.insert(Tag::new("singleton", true, "v1"), HashSet::from([0]));
        tags.insert(Tag::new("singleton", true, "v2"), HashSet::new());
        tags.insert(Tag::from("tag1"), HashSet::new());
        let files = vec![Entry {
            source: PathBuf::from("/fake/dir/where/file/exists/file1.txt"),
        }];

        let children = get_children(
            &PathBuf::from("/singleton".to_owned() + TAG_SEPARATOR + "v2"),
            &tags,
            &files,
        )
        .collect::<HashSet<_>>();
        // Inner shows only non-singleton collisions, and related files
        assert_eq!(1, children.len());
        assert!(children.contains(&(fuse_mt::FileType::Directory, &OsString::from("tag1"))));
        assert!(!children.contains(&(fuse_mt::FileType::RegularFile, &OsString::from("file1.txt"))));
    }

    #[traced_test]
    #[test]
    fn unlink_present_file() {
        let _m = MTX.lock();

        let ctx = MockLibcWrapper::new_context();
        ctx.expect().returning(|| {
            let mut mock = MockLibcWrapper::default();
            mock.expect_unlink().times(1).returning(|_path| Ok(()));
            mock
        });
        let mut fs = TagFS::<MockLibcWrapper>::new();
        let mut tags = HashSet::new();
        tags.insert(Tag::from("tag"));
        fs.add_file(&PathBuf::from("/fake/source/present.txt"), tags);
        let r = fs.unlink(
            RequestInfo {
                unique: 0,
                uid: 0,
                gid: 0,
                pid: 0,
            },
            &PathBuf::from("/tag"),
            &OsString::from("present.txt"),
        );
        assert!(r.is_ok());
    }

    #[traced_test]
    #[test]
    fn unlink_missing_file() {
        let _m = MTX.lock();

        let ctx = MockLibcWrapper::new_context();
        ctx.expect().returning(MockLibcWrapper::default);
        let mut fs = TagFS::<MockLibcWrapper>::new();
        let mut tags = HashSet::new();
        tags.insert(Tag::from("tag"));
        fs.add_file(&PathBuf::from("/fake/source/present.txt"), tags);
        let r = fs.unlink(
            RequestInfo {
                unique: 0,
                uid: 0,
                gid: 0,
                pid: 0,
            },
            &PathBuf::from("/tag"),
            &OsString::from("missing.txt"),
        );
        assert!(r.is_err());
        assert_eq!(ENOENT, r.unwrap_err());
    }

    #[traced_test]
    #[test]
    fn unlink_forbidden_file() {
        let _m = MTX.lock();

        let ctx = MockLibcWrapper::new_context();
        ctx.expect().returning(|| {
            let mut mock = MockLibcWrapper::default();
            mock.expect_unlink().times(1).returning(|path| {
                if path == PathBuf::from("/fake/source/present.txt") {
                    Err(std::io::Error::from_raw_os_error(EPERM))
                } else {
                    Err(std::io::Error::from_raw_os_error(ENOENT))
                }
            });
            mock
        });
        let mut fs = TagFS::<MockLibcWrapper>::new();
        let mut tags = HashSet::new();
        tags.insert(Tag::from("tag"));
        fs.add_file(&PathBuf::from("/fake/source/present.txt"), tags);
        let r = fs.unlink(
            RequestInfo {
                unique: 0,
                uid: 0,
                gid: 0,
                pid: 0,
            },
            &PathBuf::from("/tag"),
            &OsString::from("present.txt"),
        );
        assert!(r.is_err());
        assert_eq!(EPERM, r.unwrap_err());
        let r = fs.unlink(
            RequestInfo {
                unique: 0,
                uid: 0,
                gid: 0,
                pid: 0,
            },
            &PathBuf::from("/tag"),
            &OsString::from("missing.txt"),
        );
        assert!(r.is_err());
        assert_eq!(ENOENT, r.unwrap_err());
    }
}
