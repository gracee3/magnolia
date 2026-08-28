//! Authoritative Magnolia application service.

mod client;
mod persistence;
mod runtime_port;
mod service;

pub use client::*;
pub use persistence::*;
pub use runtime_port::*;
pub use service::*;
