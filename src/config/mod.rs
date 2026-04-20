use crate::system::pkg::{PackageManager, PackageSource, Repository};

pub mod aliases;
pub mod expr;
mod parser;

mod steps;
mod validate;

pub use steps::*;

pub fn load_steps(file: &str) -> anyhow::Result<Vec<Step>> {
    let steps = parser::parse(file)?;
    validate::validate(&steps)?;
    Ok(steps)
}
