use std::io::{Read, Write};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::{Guard, confine, spawn};
#[cfg(windows)]
pub use windows::{Guard, confine, spawn};

pub type Pipes = (Box<dyn Write + Send>, Box<dyn Read + Send>, Guard);
