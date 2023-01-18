
pub mod error;
pub mod context;
pub mod task;

use context::Scheduler;
use task::Task;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let mut s = Scheduler::new(16);
        s.spawn( async { Ok(()) } );
        s.wait_for_empty();
    }
}
