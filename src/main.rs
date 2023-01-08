use clap::Parser;

mod error;
mod options;
mod traverse;

fn main() -> Result<(), error::E> {
    let mut t = traverse::Traverser {
        opt: options::Options::parse(),
    };

    traverse::traverse(&mut t)?;

    Ok(())
}
