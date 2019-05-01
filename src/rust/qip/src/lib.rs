#![warn(clippy::all)]

pub mod fail;
pub mod prelude;
pub mod result;

use prelude::*;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
