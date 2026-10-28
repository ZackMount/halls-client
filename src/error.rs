#[allow(unused_imports)]
use crate::prelude::*;
use core::fmt;

#[derive(Debug)]
pub struct Error(String);
impl Error {
    pub fn message(&self) -> &str {
        &self.0
    }
    pub fn new(message: String) -> Self {
        Self(message)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl core::error::Error for Error {}
#[cfg(feature = "tools")]
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self(error.to_string())
    }
}
impl From<core::num::ParseIntError> for Error {
    fn from(e: core::num::ParseIntError) -> Self {
        Self(e.to_string())
    }
}
impl From<core::num::ParseFloatError> for Error {
    fn from(e: core::num::ParseFloatError) -> Self {
        Self(e.to_string())
    }
}
pub type Result<T> = core::result::Result<T, Error>;

pub trait Context<T> {
    fn context(self, message: &str) -> Result<T>;
    fn with_context(self, message: impl FnOnce() -> String) -> Result<T>;
}
impl<T, E: fmt::Display> Context<T> for core::result::Result<T, E> {
    fn context(self, message: &str) -> Result<T> {
        self.map_err(|error| Error(format!("{message}: {error}")))
    }
    fn with_context(self, message: impl FnOnce() -> String) -> Result<T> {
        self.map_err(|error| Error(format!("{}: {error}", message())))
    }
}
impl<T> Context<T> for Option<T> {
    fn context(self, message: &str) -> Result<T> {
        self.ok_or_else(|| Error(message.into()))
    }
    fn with_context(self, message: impl FnOnce() -> String) -> Result<T> {
        self.ok_or_else(|| Error(message()))
    }
}

#[macro_export]
macro_rules! bail {
    ($($arg:tt)*) => { return Err($crate::error::Error::new(format!($($arg)*))) };
}
pub use crate::bail;
