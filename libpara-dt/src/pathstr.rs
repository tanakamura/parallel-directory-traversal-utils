use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

pub fn entry_to_path<'a>(e: &'a nix::dir::Entry) -> &'a std::path::Path {
    let name = e.file_name();
    let osstr_name = OsStr::from_bytes(name.to_bytes());
    std::path::Path::new(osstr_name)
}
