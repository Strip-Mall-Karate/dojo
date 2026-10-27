use std::result;

pub type Result<T> = result::Result<T, anyhow::Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("A primary service package could not be determined from: {0:?}. At least one \
             package with a run hook must be provided.")]
    PrimaryServicePackageNotFound(Vec<String>),
}
