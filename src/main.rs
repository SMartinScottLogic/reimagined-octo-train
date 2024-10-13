use anyhow::{Context as _, Result};
use clap::Parser;
use filesystem::TagFS;
use std::ffi::OsStr;
use std::str::FromStr;
use std::{env, os::unix::fs::MetadataExt};
use tracing::{debug, error, info, Level};
use tracing_subscriber::fmt::format::FmtSpan;

mod filesystem;

#[derive(Parser, Debug)]
#[command(
    version,
    about("Tag-based filesystem"),
    after_help = "Tag-based filesystem, with directory hierarchy based on intrinsic file properties."
)]
struct Args {
    /// Mount point
    mountpoint: String,

    /// Source folder
    source: String,

    /// Number of threads
    #[arg(short, long, default_value_t = 1)]
    num_threads: usize,
}

fn setup_logger() {
    // install global collector configured based on RUST_LOG env var.
    let level =
        env::var("RUST_LOG").map_or(Level::INFO, |v| Level::from_str(&v).unwrap_or(Level::INFO));
    tracing_subscriber::fmt()
        .with_span_events(FmtSpan::ACTIVE)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true)
        .with_max_level(level)
        .with_ansi(false)
        .init();
}
fn main() -> Result<()> {
    setup_logger();
    let args = Args::parse();

    let cookie = magic::Cookie::open(magic::cookie::Flags::ERROR | magic::cookie::Flags::MIME_TYPE)
        .context("open libmagic database")?;
    let cookie = match cookie.load(&Default::default()) {
        Ok(v) => Ok(v),
        Err(e) => Err(anyhow::Error::msg(format!("load libmagic database: {e}"))),
    }?;

    for e in walkdir::WalkDir::new(args.source)
        .same_file_system(true)
        .into_iter()
        .flatten()
    {
        debug!(entry = debug(&e), "walkdir");
        if e.file_type().is_file() {
            let metadata = e.metadata().unwrap();
            let size = metadata.size();
            let t = cookie.file(e.path()).ok();
            info!(filename = ?e.path(), size, ?t, "file");
        }
    }

    let target_fs = TagFS::new();

    let fuse_args: Vec<&OsStr> = vec![OsStr::new("-o"), OsStr::new("auto_unmount")];
    fuse_mt::mount(
        fuse_mt::FuseMT::new(target_fs, args.num_threads),
        &args.mountpoint,
        &fuse_args,
    )
    .context("running filesystem")
}
