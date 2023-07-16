// Copyright (C) benaryorg <binary@benary.org>
//
// This software is licensed as described in the file COPYING, which
// you should have received as part of this distribution.
//
// SPDX-License-Identifier: AGPL-3.0-or-later

mod responder;
mod unix;
mod pipe;
mod query;

pub use responder::Responder;
pub use unix::Unix;
pub use pipe::Pipe;
