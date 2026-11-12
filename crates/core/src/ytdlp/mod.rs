//! yt-dlp integration: argument builders, output parsing, error classification.

pub mod args;
pub mod download_args;
pub mod errors;
pub mod finalize;
pub mod parse;
pub mod process;
pub mod progress;
pub mod runner;
pub mod subs;

#[cfg(test)]
mod tests_args;
#[cfg(test)]
mod tests_parse;
#[cfg(test)]
mod tests_subs;
