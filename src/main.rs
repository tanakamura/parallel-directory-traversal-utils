use clap::Parser;

mod dir;
mod error;
mod events;
mod options;
mod pathstr;
mod traverse;

fn main() -> Result<(), error::E> {
    let mut t = traverse::Traverser {
        opt: options::Options::parse(),
    };

    traverse::traverse(&mut t)?;

    Ok(())
}
