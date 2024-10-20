use anyhow::{Context as _, Result};
use clap::Parser;
use filesystem::TagFS;
use std::env;
use std::ffi::OsStr;
use std::path::Path;
use std::str::FromStr;
use std::collections::HashSet;
use tagger::{MetadataTagger, MimeTagger, Tag, Tagger};
use tracing::{debug, info, Level};
use tracing_subscriber::fmt::format::FmtSpan;

mod filesystem;
mod tagger;

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

#[derive(Debug)]
struct FileUpdater {
    taggers: Vec<Box<dyn Tagger>>,
}
impl FileUpdater {
    fn new() -> Self {
        Self {
            taggers: Vec::new(),
        }
    }

    fn add_tagger(&mut self, tagger: impl Tagger + 'static) {
        self.taggers.push(Box::new(tagger));
    }

    fn tag(&self, path: &Path) -> HashSet<Tag> {
        self.taggers.iter().fold(HashSet::new(), |mut acc, tagger| {
            match tagger.tag(path) {
                Ok(tags) => acc.extend(tags),
                Err(_) => todo!(),
            }
            acc
        })
    }
}

fn main() -> Result<()> {
    setup_logger();
    let args = Args::parse();

    let mut target_fs = TagFS::new();
    let mut file_updater = FileUpdater::new();
    file_updater.add_tagger(MimeTagger::new());
    file_updater.add_tagger(MetadataTagger::new());

    for e in walkdir::WalkDir::new(args.source)
        .same_file_system(true)
        .into_iter()
        .flatten()
    {
        debug!(entry = debug(&e), "walkdir");
        if e.file_type().is_file() {
            target_fs.add_file(e.path(), file_updater.tag(e.path()));
            info!(filename = ?e.path(), "file");
        }
    }

    info!(?target_fs, "scanned");

    let fuse_args: Vec<&OsStr> = vec![OsStr::new("-o"), OsStr::new("auto_unmount")];
    fuse_mt::mount(
        fuse_mt::FuseMT::new(target_fs, args.num_threads),
        &args.mountpoint,
        &fuse_args,
    )
    .context("running filesystem")
}
