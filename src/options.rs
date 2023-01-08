use clap::{Parser};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize, Debug, clap::Subcommand)]
pub enum Method {
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

#[derive(Clone, Debug, Serialize, Deserialize, clap::ValueEnum)]
pub enum Order {
    Alphabetical,
    Readdir,
    Unordered
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
    #[clap(value_enum)]
    pub order: Order,
    #[arg(long, default_value_t = 4)]
    pub num_threads: usize,

    #[command(subcommand)]
    pub method: Method,
}
