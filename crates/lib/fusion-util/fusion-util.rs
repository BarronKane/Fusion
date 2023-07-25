

use std::env;
use std::io;
use std::path::PathBuf;

/// Returns the directory from which the executable exists.
pub fn get_cwd() -> io::Result<PathBuf> {
    let mut pwd = env::current_exe()?;
    pwd.pop();
    Ok(pwd)
}

/// Returns the directory and filename from which the executable exists.
pub fn get_main() -> io::Result<PathBuf> {
    let exe = env::current_exe()?;
    Ok(exe)
}

/// Reads a file and returns a [Vec<u8>] of the entire file.
pub fn file_to_bytes(path: std::path::PathBuf) -> Vec<u8> {
    let bytes = std::fs::read(path).unwrap();
    return bytes;
}

/// Reads a file and returns a [String] of the contents of the entire file.
pub fn file_to_string(path: std::path::PathBuf) -> String {
    std::fs::read_to_string(path).unwrap()
}
