use crate::system::pkg::{PackageManager, PackageSource, Repository};

pub mod aliases;
pub mod expr;
mod parser;

mod steps;
mod validate;

pub use crate::config::validate::ValidationMode;
pub use steps::*;

pub fn load_steps(file: &str, mode: ValidationMode) -> anyhow::Result<Vec<Step>> {
    let steps = parser::parse(file)?;
    validate::validate(&steps, mode)?;
    Ok(steps)
}
