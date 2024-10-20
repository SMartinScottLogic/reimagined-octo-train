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
use tracing::{debug, info};

use crate::tagger::Tag;

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
    //tags: HashSet<OsString>,
    tags: HashMap<Tag, HashSet<usize>>,
}

impl<'a> TagFS {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            tags: HashMap::new(),
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
        self.tags.keys().any(|t| t.as_os_str() == tag)
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
            Component::Prefix(_prefix_component) => todo!(),
            Component::RootDir => true,
            Component::CurDir => false,
            Component::ParentDir => false,
            Component::Normal(tag) => self.contains_tag(tag),
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

        for (child_type, child_name) in Self::get_children(path, &self.tags, &self.files) {
            info!(?child_type, name = ?child_name, "children");
            entries.push(DirectoryEntry {
                name: child_name.into(),
                kind: child_type,
            });
        }

        Ok(entries)
    }
}

impl TagFS {
    fn get_children<'a, 'b, 'c>(
        root: &Path,
        tags: &'a HashMap<Tag, HashSet<usize>>,
        files: &'b [Entry],
    ) -> impl Iterator<Item = (FileType, &'c OsStr)>
    where
        'a: 'c,
        'b: 'c,
    {
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

        info!(?file_ids, ?root_tags, ?root, "residue");

        tags.iter()
            // Filter out tags already in path
            .filter(move |(t, _)| !root_tags.contains(t.as_os_str()))
            // TODO Filter out already seen filter tags
            .filter(|(t, _)| {
                info!(?t, "non-matched tag");
                true
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
}

#[cfg(test)]
mod test {
    use std::{
        collections::{HashMap, HashSet}, ffi::OsString, path::PathBuf
    };

    use tracing_test::traced_test;

    use crate::tagger::{Tag, TAG_SEPARATOR};

    use super::{Entry, TagFS};

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

        let children = TagFS::get_children(&PathBuf::from("/"), &tags, &files);
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

        let children = TagFS::get_children(&PathBuf::from("/tag2"), &tags, &files);
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

        let children = TagFS::get_children(&PathBuf::from("/tag2/tag1"), &tags, &files);
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

        let children = TagFS::get_children(&PathBuf::from("/"), &tags, &files).collect::<HashSet<_>>();
        // Root shows all tags, no files
        assert_eq!(3, children.len());
        assert!(children.contains(&(fuse_mt::FileType::Directory, &OsString::from("singleton".to_owned() + TAG_SEPARATOR + "v1"))));
        assert!(children.contains(&(fuse_mt::FileType::Directory, &OsString::from("singleton".to_owned() + TAG_SEPARATOR + "v2"))));
        assert!(children.contains(&(fuse_mt::FileType::Directory, &OsString::from("tag1"))));
    }

    #[traced_test]
    #[test]
    fn get_children_singleton_child() {
        let mut tags = HashMap::new();
        tags.insert(Tag::new("singleton", true, "v1"), HashSet::new());
        tags.insert(Tag::new("singleton", true, "v2"), HashSet::new());
        tags.insert(Tag::from("tag1"), HashSet::new());
        let files = vec![Entry {
            source: PathBuf::from("/fake/dir/where/file/exists/file1.txt"),
        }];

        let children = TagFS::get_children(&PathBuf::from("/singleton".to_owned() + TAG_SEPARATOR + "v1"), &tags, &files).collect::<HashSet<_>>();
        // Inner shows only non-singleton collisions, and related files
        assert_eq!(1, children.len());
        assert!(children.contains(&(fuse_mt::FileType::Directory, &OsString::from("tag1"))));

    }
}
