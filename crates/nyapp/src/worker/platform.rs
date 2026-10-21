use std::io::{Read, Write};

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub(crate) use windows::delete_profile;
#[cfg(windows)]
pub use windows::{Guard, confine, spawn};

pub type Pipes = (Box<dyn Write + Send>, Box<dyn Read + Send>, Guard);
