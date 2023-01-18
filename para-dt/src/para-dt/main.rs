use clap::Parser;
use libpara_dt::traverse;
use libpara_dt::error;
use libpara_dt::options;

fn main() -> Result<(), error::E> {
    let mut t = traverse::Traverser {
        opt: options::Options::parse(),
    };

    traverse::traverse(&mut t)?;

    Ok(())
}
