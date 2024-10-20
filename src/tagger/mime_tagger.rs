use std::{collections::HashSet, path::Path};

use anyhow::Context as _;
use magic::{cookie::Load, Cookie};
use tracing::error;

use super::{Error, Tag, Tagger};

#[derive(Debug)]
pub struct MimeTagger {
    cookie: Cookie<Load>,
}
impl MimeTagger {
    pub fn new() -> Self {
        let cookie =
            magic::Cookie::open(magic::cookie::Flags::ERROR | magic::cookie::Flags::MIME_TYPE)
                .context("open libmagic database")
                .unwrap();
        let cookie = cookie.load(&Default::default()).unwrap();

        Self { cookie }
    }
}
impl Tagger for MimeTagger {
    fn tag(&self, path: &Path) -> Result<HashSet<Tag>, Error> {
        self.cookie
            .file(path)
            .map(|tag| HashSet::from([Tag::new("mime", true, tag.replace('/', "|"))]))
            .map_err(|e| {
                error!(error = ?e, "get mime type");
                Error::Illegible
            })
    }
}
