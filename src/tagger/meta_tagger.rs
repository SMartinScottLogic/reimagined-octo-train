use std::{collections::HashSet, ffi::OsString, os::unix::fs::MetadataExt as _, path::Path};

use time::OffsetDateTime;
use tracing::error;

use super::{Tag, Tagger};

#[derive(Debug)]
pub struct MetadataTagger {}
impl MetadataTagger {
    pub fn new() -> Self {
        Self {}
    }
}
impl Tagger for MetadataTagger {
    fn tag(&self, path: &Path) -> HashSet<Tag> {
        let mut tags = HashSet::new();
        match path.metadata() {
            Ok(metadata) if metadata.is_file() => {
                tags.insert(Tag::new("size", metadata.size().to_string()));
                if let Ok(date) = metadata.modified() {
                    let t: OffsetDateTime = date.into();
                    tags.insert(Tag::new(
                        "modified",
                        format!(
                            "{:0>4}-{:0>2}-{:0>2} {:0>2}:{:0>2}:{:0>2}",
                            t.year(),
                            t.month() as u8,
                            t.day(),
                            t.hour(),
                            t.minute(),
                            t.second()
                        ),
                    ));
                }
            }
            Ok(_) => error!("non-file for metadata"),
            Err(e) => error!(error = ?e, "get file metadata"),
        };
        tags
    }
}
