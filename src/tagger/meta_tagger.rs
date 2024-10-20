use std::{collections::HashSet, os::unix::fs::MetadataExt as _, path::Path};

use time::OffsetDateTime;
use tracing::error;

use super::{Tag, Tagger, Error};

#[derive(Debug)]
pub struct MetadataTagger {}
impl MetadataTagger {
    pub fn new() -> Self {
        Self {}
    }
}
impl Tagger for MetadataTagger {
    fn tag(&self, path: &Path) -> Result<HashSet<Tag>, Error> {
        let mut tags = HashSet::new();
        match path.metadata() {
            Ok(metadata) if metadata.is_file() => {
                tags.insert(Tag::new("size", true, metadata.size().to_string()));
                if let Ok(date) = metadata.modified() {
                    let t: OffsetDateTime = date.into();
                    tags.insert(Tag::new(
                        "modified",
                        true,
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
            Err(e) => {
                error!(error = ?e, "get file metadata");
                return Err(Error::Illegible)
            }
        };
        Ok(tags)
    }
}

#[cfg(test)]
mod test {
    use std::{fs, io, path::PathBuf, time::{Duration, SystemTime}};

    use crate::tagger::{Error, Tag, Tagger};

    use super::MetadataTagger;

    #[test]
    fn tags() -> io::Result<()>{
        let path = PathBuf::from("test_file");
        let file = fs::File::create_new("test_file")?;
        file.set_len(1234)?;
        // Set modified time to midnight on 01/Jan/1970
        file.set_modified(SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(24 * 60 * 60)).unwrap())?;

        let tagger = MetadataTagger::new();
        let tags = tagger.tag(&path).unwrap();
        assert_eq!(2, tags.len());
        assert!(tags.contains(&Tag::new("size", true, "1234")));
        assert!(tags.contains(&Tag::new("modified", true, "1970-01-02 00:00:00")));
        fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn tags_dir() {
        let path = PathBuf::from("src");
        let tagger = MetadataTagger::new();
        let tags = tagger.tag(&path).unwrap();
        assert!(tags.is_empty());
    }

    #[test]
    fn tags_missing() {
        let path = PathBuf::from("test_file");
        let tagger = MetadataTagger::new();
        let tags = tagger.tag(&path);
        assert!(tags.is_err_and(|e| e == Error::Illegible));
    }
}