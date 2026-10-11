// pub fn add(left: u64, right: u64) -> u64 {
//     left + right
// }

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn it_works() {
//         let result = add(2, 2);
//         assert_eq!(result, 4);
//     }
// }

//! Soirvu core: everything that does not depend on Tauri.

pub mod crypto;
pub mod error;
pub mod vault;

pub use error::{Error, Result};
