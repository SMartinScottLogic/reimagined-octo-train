use std::{io, mem::MaybeUninit};

use fuse_mt::FileType;
use tracing::error;

pub(crate) fn mode_to_filetype(mode: libc::mode_t) -> FileType {
    match mode & libc::S_IFMT {
        libc::S_IFDIR => FileType::Directory,
        libc::S_IFREG => FileType::RegularFile,
        libc::S_IFLNK => FileType::Symlink,
        libc::S_IFBLK => FileType::BlockDevice,
        libc::S_IFCHR => FileType::CharDevice,
        libc::S_IFIFO => FileType::NamedPipe,
        libc::S_IFSOCK => FileType::Socket,
        _ => {
            panic!("unknown file type");
        }
    }
}

pub(crate) fn fstat(fh: u64) -> io::Result<libc::stat> {
    let mut stat = MaybeUninit::<libc::stat>::uninit();

    let result = unsafe { libc::fstat(fh as libc::c_int, stat.as_mut_ptr()) };
    if -1 == result {
        let e = io::Error::last_os_error();
        error!(fh, ?e, "fstat");
        Err(e)
    } else {
        let stat = unsafe { stat.assume_init() };
        Ok(stat)
    }
}
