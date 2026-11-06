//! The Unstrung core service: storage, the chat engine and chat logic.
//! It has no UI code, so the app and the tests use the same paths.

#![forbid(unsafe_code)]

pub mod avatar;
pub mod backup;
pub mod card;
pub mod catalog;
pub mod chat;
pub mod content;
pub mod db;
pub mod download;
pub mod engine;
pub mod engine_setup;
pub mod error;
pub mod files;
pub mod guard;
pub mod hardware;
pub mod library;
pub mod location;
pub mod memory;
pub mod models;
pub mod ollama;
pub mod paths;
pub mod pictures;
pub mod presets;
pub mod prompt;
pub mod relocate;
pub mod repair;
pub mod sidecar;
pub mod start;
pub mod starters;
pub mod storage;
pub mod uninstall;
pub mod vault;

pub use error::{Error, Result};
