mod meta_tagger;
mod mime_tagger;

use std::{collections::HashSet, ffi::OsString, fmt::Debug, path::Path};

pub use meta_tagger::MetadataTagger;
pub use mime_tagger::MimeTagger;

pub trait Tagger: Debug {
    fn tag(&self, path: &Path) -> HashSet<OsString>;
}
