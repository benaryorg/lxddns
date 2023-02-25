#![allow(clippy::collapsible_else_if)]

pub mod error;
pub mod lxd;
pub mod pdns;
mod pdns_io;
mod amqp;

pub use amqp::responder::Responder as AmqpResponder;
pub use amqp::unix::Unix as AmqpUnix;
pub use amqp::pipe::Pipe as AmqpPipe;

