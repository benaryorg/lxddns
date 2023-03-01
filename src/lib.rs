#![allow(clippy::collapsible_else_if)]

pub mod error;
pub mod lxd;
pub mod pdns;
mod pdns_io;

#[cfg(feature = "amqp")]
pub mod amqp;
#[cfg(feature = "http")]
pub mod http;
