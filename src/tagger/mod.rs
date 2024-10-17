mod meta_tagger;
mod mime_tagger;

use std::{
    collections::HashSet,
    ffi::{OsStr, OsString},
    fmt::Debug,
    path::Path,
};

pub use meta_tagger::MetadataTagger;
pub use mime_tagger::MimeTagger;

const TAG_SEPARATOR: &str = ":";

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Tag {
    label: Option<OsString>,
    value: OsString,
    display: OsString,
}
impl Tag {
    pub(crate) fn new(label: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        let label: OsString = label.into();
        let value: OsString = value.into();
        let mut display = label.clone();
        display.push(TAG_SEPARATOR);
        display.push(&value);
        Self {
            label: Some(label),
            value,
            display,
        }
    }
    pub(crate) fn as_os_str(&self) -> &OsStr {
        self.display.as_os_str()
    }
}
impl From<OsString> for Tag {
    fn from(value: OsString) -> Self {
        Self {
            label: None,
            display: value.clone(),
            value,
        }
    }
}
impl From<&str> for Tag {
    fn from(value: &str) -> Self {
        let value: OsString = value.into();
        Self::from(value)
    }
}
pub trait Tagger: Debug {
    fn tag(&self, path: &Path) -> HashSet<Tag>;
}

#[cfg(test)]
mod test {
    use std::ffi::OsString;

    use crate::tagger::TAG_SEPARATOR;

    use super::Tag;

    #[test]
    fn as_os_str_no_label() {
        let tag = Tag::from("test");
        assert_eq!(OsString::from("test").as_os_str(), tag.as_os_str());
    }

    #[test]
    fn as_os_str() {
        let tag = Tag::new("label", "value");
        let expected: OsString = format!("{}{}{}", "label", TAG_SEPARATOR, "value").into();
        assert_eq!(expected.as_os_str(), tag.as_os_str());
    }
}
