//! yt-dlp integration: argument builders, output parsing, error classification.

pub mod args;
pub mod download_args;
pub mod errors;
pub mod parse;
pub mod process;
pub mod progress;
pub mod runner;

#[cfg(test)]
mod tests_args;
#[cfg(test)]
mod tests_parse;
