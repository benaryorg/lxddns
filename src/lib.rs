// Copyright (C) benaryorg <binary@benary.org>
//
// This software is licensed as described in the file COPYING, which
// you should have received as part of this distribution.
//
// SPDX-License-Identifier: AGPL-3.0-or-later

#![allow(clippy::collapsible_else_if)]

pub mod error;
pub mod lxd;
pub mod pdns;
mod pdns_io;

#[cfg(feature = "amqp")]
pub mod amqp;
#[cfg(feature = "http")]
pub mod http;
