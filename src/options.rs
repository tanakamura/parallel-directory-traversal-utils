use clap::Parser;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Clone, Serialize, Deserialize, Debug, clap::Subcommand, Eq, PartialEq)]
pub enum Method {
    ///
    DryRun,

    /// dump all file paths. like `find .`
    List,
    /// count file size. like `du`
    DU {
        /// Count each inode object. Uncount second and subsequent hard link.
        #[arg(long, default_value_t = true)]
        count_inode: bool,
    },
    DumpSTAT {
        #[arg(long, default_value_t = false)]
        get_xattr: bool,
    },
    CloneDirectory {
        #[arg(long)]
        dst: PathBuf,
        #[arg(long)]
        use_o_direct: bool,
        #[arg(long, default_value_t = true)]
        use_fallocate: bool,
        #[arg(long, default_value_t = 1024*1024)]
        buffer_byte_size: u64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, clap::ValueEnum, Eq, PartialEq)]
pub enum Order {
    Alphabetical,
    Readdir,
    Unordered,
}

#[derive(Clone, Serialize, Deserialize, Debug, Parser)]
#[command(about)]
pub struct Options {
    #[arg(long)]
    pub src_path: PathBuf,
    #[arg(long, default_value_t = 64)]
    pub readdir_dirent_buffer_size: usize,
    #[arg(long, default_value_t = 32)]
    pub max_ioreq_depth: usize,
    #[arg(long)]
    pub follow_symlink: bool,
    #[arg(long, value_enum, default_value_t = Order::Alphabetical)]
    pub order: Order,
    #[arg(long, default_value_t = 4)]
    pub num_threads: usize,
    #[arg(long, default_value_t = false)]
    pub ignore_eaccess: bool,

    #[command(subcommand)]
    pub method: Method,
}

pub fn test_option(path: &str) -> Options {
    Options {
        src_path: PathBuf::from_str(path).unwrap(),
        readdir_dirent_buffer_size: 64,
        max_ioreq_depth: 32,
        follow_symlink: false,
        order: Order::Unordered,
        num_threads: 1,
        method: Method::List,
        ignore_eaccess: true,
    }
}
