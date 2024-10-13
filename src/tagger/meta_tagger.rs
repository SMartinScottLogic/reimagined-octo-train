use std::{collections::HashSet, ffi::OsString, os::unix::fs::MetadataExt as _, path::Path};

use tracing::error;

use super::Tagger;

#[derive(Debug)]
pub struct MetadataTagger {}
impl MetadataTagger {
    pub fn new() -> Self {
        Self {}
    }
}
impl Tagger for MetadataTagger {
    fn tag(&self, path: &Path) -> HashSet<OsString> {
        let mut tags = HashSet::new();
        match path.metadata() {
            Ok(metadata) if metadata.is_file() => {
                tags.insert(format!("size={}", metadata.size()).into());
            }
            Ok(_) => error!("non-file for metadata"),
            Err(e) => error!(error = ?e, "get file metadata"),
        };
        tags
    }
}
