use std::{collections::HashSet, path::Path};

use anyhow::Context;
use magic::{cookie::Load, Cookie};
use tracing::error;

use super::{Error, Tag, Tagger};

pub(crate) trait MimeExtractor {
    fn new() -> Self;
    fn file(&self, filename: &Path) -> Result<String, anyhow::Error>;
}

impl MimeExtractor for Cookie<Load> {
    fn new() -> Self {
        let cookie =
            magic::Cookie::open(magic::cookie::Flags::ERROR | magic::cookie::Flags::MIME_TYPE)
                .context("open libmagic database")
                .unwrap();
        cookie.load(&Default::default()).unwrap()
    }
    fn file(&self, filename: &Path) -> Result<String, anyhow::Error> {
        self.file(filename).context("mime lookup")
    }
}
#[derive(Debug)]
pub struct MimeTagger<T> {
    mime_extractor: T,
}
impl<T: MimeExtractor> MimeTagger<T> {
    pub fn new() -> Self {
        Self {
            mime_extractor: T::new(),
        }
    }
}
impl<T: MimeExtractor + std::fmt::Debug> Tagger for MimeTagger<T> {
    fn tag(&self, path: &Path) -> Result<HashSet<Tag>, Error> {
        self.mime_extractor
            .file(path)
            .map(|tag| HashSet::from([Tag::new("mime", true, tag.replace('/', "|"))]))
            .map_err(|e| {
                error!(error = ?e, "get mime type");
                Error::Illegible
            })
    }
}

#[cfg(test)]
mod test {
    use std::{
        collections::HashSet,
        ffi::OsString,
        path::{Path, PathBuf},
    };

    use anyhow::Context;

    use magic::{cookie::Load, Cookie};
    use tracing::debug;
    use tracing_test::traced_test;

    use crate::tagger::{Tag, Tagger as _, TAG_SEPARATOR};

    use super::{MimeExtractor, MimeTagger};

    #[traced_test]
    #[test]
    fn mime_extraction_success() {
        #[derive(Debug)]
        struct TestExtractor {}
        impl MimeExtractor for TestExtractor {
            fn new() -> Self {
                Self {}
            }
            fn file(&self, _filename: &Path) -> Result<String, anyhow::Error> {
                Ok(String::from("Ok"))
            }
        }
        let t = MimeTagger::<TestExtractor>::new();
        assert!(t.tag(&PathBuf::from("bob")).is_ok_and(|v| {
            debug!(?v);
            v.len() == 1
                && v.iter().next().unwrap().as_os_str()
                    == OsString::from("mime".to_owned() + TAG_SEPARATOR + "Ok")
        }));
    }

    #[traced_test]
    #[test]
    fn mime_extraction_failed() {
        #[derive(Debug)]
        struct TestExtractor {}
        impl MimeExtractor for TestExtractor {
            fn new() -> Self {
                Self {}
            }
            fn file(&self, _filename: &Path) -> Result<String, anyhow::Error> {
                Err(std::io::Error::from_raw_os_error(0)).context("test")
            }
        }
        let t = MimeTagger::<TestExtractor>::new();
        assert!(t.tag(&PathBuf::from("bob")).is_err_and(|e| {
            debug!(?e);
            e == super::Error::Illegible
        }));
    }

    #[traced_test]
    #[test]
    fn mime_extraction_real() {
        let t = MimeTagger::<Cookie<Load>>::new();
        let t = t.tag(&PathBuf::from("./src/main.rs"));
        assert!(t.is_ok());
        let t = t.unwrap();
        assert_eq!(t, HashSet::from([Tag::new("mime", true, "text|x-c")]));
    }
}
