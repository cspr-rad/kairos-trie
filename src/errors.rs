use alloc::{
    boxed::Box,
    string::{String, ToString},
};
use core::fmt::{self, Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TrieError(Box<str>);

impl TrieError {
    #[inline]
    pub fn display(&self) -> &str {
        &self.0
    }
}

impl Display for TrieError {
    #[inline]
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for TrieError {
    #[inline]
    fn from(s: &str) -> Self {
        Self(s.into())
    }
}

impl From<String> for TrieError {
    #[inline]
    fn from(s: String) -> Self {
        Self(s.into_boxed_str())
    }
}

impl From<&String> for TrieError {
    #[inline]
    fn from(s: &String) -> Self {
        Self(s.clone().into_boxed_str())
    }
}

impl From<&TrieError> for String {
    #[inline]
    fn from(e: &TrieError) -> Self {
        e.0.to_string()
    }
}

impl From<TrieError> for String {
    #[inline]
    fn from(e: TrieError) -> Self {
        e.0.to_string()
    }
}
