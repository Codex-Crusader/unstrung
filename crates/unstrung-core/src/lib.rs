//! The Unstrung core service: storage, the chat engine and chat logic.
//! It has no UI code, so the app and the tests use the same paths.

#![forbid(unsafe_code)]

pub mod avatar;
pub mod card;
pub mod catalog;
pub mod chat;
pub mod db;
pub mod engine;
pub mod error;
pub mod guard;
pub mod hardware;
pub mod library;
pub mod models;
pub mod ollama;
pub mod paths;
pub mod presets;
pub mod prompt;
pub mod sidecar;
pub mod starters;

pub use error::{Error, Result};
