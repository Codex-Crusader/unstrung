//! The Unstrung core service: storage, the chat engine and chat logic.
//! It has no UI code, so the app and the tests use the same paths.

#![forbid(unsafe_code)]

pub mod chat;
pub mod db;
pub mod engine;
pub mod error;
pub mod ollama;
pub mod paths;
pub mod prompt;
pub mod sidecar;
pub mod starters;

pub use error::{Error, Result};
