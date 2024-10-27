#![allow(dead_code)]
use std::{
    ffi::CString,
    io,
    mem::MaybeUninit,
    os::unix::prelude::OsStrExt,
    path::{Path, PathBuf},
};

use fuse_mt::FileType;
use libc::c_void;
use mockall::automock;
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

#[automock]
pub trait LibcWrapper: std::fmt::Debug {
    fn new() -> Self;
    fn statfs(&self, path: PathBuf) -> io::Result<libc::statfs>;
    fn fstat(&self, fh: u64) -> io::Result<libc::stat>;
    fn lstat(&self, path: &Path) -> io::Result<libc::stat>;
    fn open(&self, path: &Path, flags: i32) -> io::Result<i32>;
    fn close(&self, fd: i32) -> io::Result<()>;
    fn read(&self, fd: i32, offset: i64, count: u32) -> io::Result<Vec<u8>>;
    fn unlink(&self, path: &Path) -> io::Result<()>;
}

#[derive(Debug)]
pub struct LibcWrapperReal;
impl LibcWrapper for LibcWrapperReal {
    fn new() -> Self {
        Self
    }
    fn statfs(&self, path: PathBuf) -> io::Result<libc::statfs> {
        let mut stat = MaybeUninit::<libc::statfs>::zeroed();

        let cstr = CString::new(path.clone().into_os_string().as_bytes())?;
        let result = unsafe { libc::statfs(cstr.as_ptr(), stat.as_mut_ptr()) };

        if -1 == result {
            let e = io::Error::last_os_error();
            error!("statfs({:?}): {}", &path, e);
            Err(e)
        } else {
            let stat = unsafe { stat.assume_init() };
            Ok(stat)
        }
    }

    fn fstat(&self, fh: u64) -> io::Result<libc::stat> {
        let mut stat = MaybeUninit::<libc::stat>::uninit();

        let result = unsafe { libc::fstat(fh as libc::c_int, stat.as_mut_ptr()) };
        if -1 == result {
            let e = io::Error::last_os_error();
            error!("fstat({:?}): {}", fh, e);
            Err(e)
        } else {
            let stat = unsafe { stat.assume_init() };
            Ok(stat)
        }
    }

    fn lstat(&self, path: &Path) -> io::Result<libc::stat> {
        let mut stat = MaybeUninit::<libc::stat>::uninit();

        let cstr = CString::new(path.to_path_buf().into_os_string().as_bytes())?;
        let result = unsafe { libc::lstat(cstr.as_ptr(), stat.as_mut_ptr()) };
        if -1 == result {
            let e = io::Error::last_os_error();
            error!("lstat({:?}): {}", path, e);
            Err(e)
        } else {
            let stat = unsafe { stat.assume_init() };
            Ok(stat)
        }
    }

    fn open(&self, path: &Path, flags: i32) -> io::Result<i32> {
        let cstr = CString::new(path.to_path_buf().into_os_string().as_bytes())?;
        let result = unsafe { libc::open(cstr.as_ptr(), flags) };
        if -1 == result {
            let e = io::Error::last_os_error();
            error!("open({:?}): {}", path, e);
            Err(e)
        } else {
            Ok(result)
        }
    }

    fn close(&self, fd: i32) -> io::Result<()> {
        let result = unsafe { libc::close(fd) };
        if -1 == result {
            let e = io::Error::last_os_error();
            error!("close({:?}): {}", fd, e);
            Err(e)
        } else {
            Ok(())
        }
    }

    fn read(&self, fd: i32, offset: i64, count: u32) -> io::Result<Vec<u8>> {
        let result = unsafe { libc::lseek64(fd, offset, libc::SEEK_SET) };
        if -1 == result {
            let e = io::Error::last_os_error();
            error!("read({:?}): {}", fd, e);
            return Err(e);
        }
        let mut buf = vec![0; count.try_into().unwrap()];

        let result = unsafe {
            libc::read(
                fd,
                buf.as_mut_ptr() as *mut c_void,
                count.try_into().unwrap(),
            )
        };
        if -1 == result {
            let e = io::Error::last_os_error();
            error!("read({:?}): {}", fd, e);
            return Err(e);
        }
        Ok(buf)
    }

    fn unlink(&self, path: &Path) -> io::Result<()> {
        let cstr = CString::new(path.to_path_buf().into_os_string().as_bytes())?;
        let result = unsafe { libc::unlink(cstr.as_ptr()) };
        if -1 == result {
            let e = io::Error::last_os_error();
            error!("open({:?}): {}", path, e);
            Err(e)
        } else {
            Ok(())
        }
    }
}
