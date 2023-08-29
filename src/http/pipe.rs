// Copyright (C) benaryorg <binary@benary.org>
//
// This software is licensed as described in the file COPYING, which
// you should have received as part of this distribution.
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::
{
	error::*,
	pdns::
	{
		TtlConfig
	},
};

use ::
{
	tokio::
	{
		io::
		{
			stdin,
			stdout,
		},
	},
};

pub struct Pipe
{
	remote: Vec<String>,
	domain: String,
	hostmaster: String,
	ttl_config: TtlConfig,
}

impl Pipe
{
	pub fn builder() -> PipeBuilder
	{
		Default::default()
	}

	pub async fn run(self) -> Result<()>
	{
		debug!("[pipe] connection opened");

		let backend = super::query::RemoteQuery::new(self.remote).await?;
		let handler = crate::pdns_io::PdnsStreamHandler::new(self.domain, &self.ttl_config, self.hostmaster, backend, stdin(), stdout()).await?;
		handler.run().await?;

		debug!("[pipe] connection closed");

		Ok(())
	}
}

#[derive(Clone,Eq,PartialEq,Hash,Debug,Default)]
pub struct PipeBuilder
{
	remote: Option<Vec<String>>,
	domain: Option<String>,
	hostmaster: Option<String>,
	ttl_config: Option<TtlConfig>,
}

impl PipeBuilder
{
	pub fn remote(mut self, remote: Vec<String>) -> Self
	{
		self.remote = Some(remote);
		self
	}

	pub fn domain<S: AsRef<str>>(mut self, domain: S) -> Self
	{
		self.domain = Some(domain.as_ref().into());
		self
	}

	pub fn hostmaster<S: AsRef<str>>(mut self, hostmaster: S) -> Self
	{
		self.hostmaster = Some(hostmaster.as_ref().into());
		self
	}

	pub fn ttl_config(mut self, ttl_config: TtlConfig) -> Self
	{
		self.ttl_config = Some(ttl_config);
		self
	}

	pub async fn run(self) -> Result<()>
	{
		let remote = self.remote.map(Result::Ok).unwrap_or_else(|| bail!("no remote provided")).context(Error::InvalidConfiguration)?;
		let domain = self.domain.map(Result::Ok).unwrap_or_else(|| bail!("no domain provided")).context(Error::InvalidConfiguration)?;
		let hostmaster = self.hostmaster.map(Result::Ok).unwrap_or_else(|| bail!("no hostmaster provided")).context(Error::InvalidConfiguration)?;
		let ttl_config = self.ttl_config.unwrap_or_default();

		Pipe
		{
			remote,
			domain,
			hostmaster,
			ttl_config,
		}.run().await
	}
}

