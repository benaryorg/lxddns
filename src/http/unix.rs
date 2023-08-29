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
		TtlConfig,
	},
};

use ::
{
	tokio_stream::wrappers::UnixListenerStream,
	tokio::
	{
		net::UnixListener,
		fs::
		{
			remove_file,
			metadata,
		},
	},
	futures::
	{
		TryStreamExt,
		StreamExt,
	},
	std::
	{
		os::unix::fs::FileTypeExt,
	},
};

pub struct Unix
{
	remote: Vec<String>,
	domain: String,
	hostmaster: String,
	unixpath: String,
	ttl_config: TtlConfig,
	unix_workers: usize,
}

impl Unix
{
	pub fn builder() -> UnixBuilder
	{
		Default::default()
	}

	pub async fn run(&self) -> Result<()>
	{
		debug!("[unix] started");

		match metadata(&self.unixpath).await
		{
			Ok(metadata) =>
			{
				if metadata.file_type().is_socket()
				{
					warn!("[unix] removing potentially stale socket");
					remove_file(&self.unixpath).await?;
				}
				else
				{
					Err(Error::UnixServerError).with_context(|| format!("unix socket exists and is not a file: {}", self.unixpath))?;
				}
			},
			Err(err) =>
			{
				if err.kind() != std::io::ErrorKind::NotFound
				{
					bail!(err);
				}
			},
		}

		let listener = UnixListener::bind(&self.unixpath)?;
		info!("[http-unix] unix socket opened");

		UnixListenerStream::new(listener).map(|res| res.context(Error::UnixServerError)).try_for_each_concurrent(self.unix_workers, |stream|
		{
			let me = &self;
			async move
			{
				debug!("[unix] connection opened");

				let backend = super::query::RemoteQuery::new(self.remote.clone()).await?;
				let (read, write) = stream.into_split();
				let handler = crate::pdns_io::PdnsStreamHandler::new(&me.domain, &me.ttl_config, &me.hostmaster, backend, read, write).await?;
				handler.run().await?;

				debug!("[unix] connection closed");
				Ok(())
			}
		}).await?;

		remove_file(&self.unixpath).await?;
		debug!("[http-unix] stopped");

		Ok(())
	}

	pub async fn handle_connection(&self) -> Result<()>
	{
		Ok(())
	}
}

#[derive(Clone,Eq,PartialEq,Hash,Debug,Default)]
pub struct UnixBuilder
{
	remote: Option<Vec<String>>,
	domain: Option<String>,
	hostmaster: Option<String>,
	unixpath: Option<String>,
	ttl_config: Option<TtlConfig>,
	unix_workers: Option<usize>,
}

impl UnixBuilder
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

	pub fn unixpath(mut self, unixpath: String) -> Self
	{
		self.unixpath = Some(unixpath);
		self
	}

	pub fn ttl_config(mut self, ttl_config: TtlConfig) -> Self
	{
		self.ttl_config = Some(ttl_config);
		self
	}

	pub fn unix_workers(mut self, unix_workers: usize) -> Self
	{
		self.unix_workers = Some(unix_workers);
		self
	}

	pub async fn run(self) -> Result<()>
	{
		let remote = self.remote.map(Result::Ok).unwrap_or_else(|| bail!("no remote provided")).context(Error::InvalidConfiguration)?;
		let domain = self.domain.map(Result::Ok).unwrap_or_else(|| bail!("no domain provided")).context(Error::InvalidConfiguration)?;
		let hostmaster = self.hostmaster.map(Result::Ok).unwrap_or_else(|| bail!("no hostmaster provided")).context(Error::InvalidConfiguration)?;
		let unixpath = self.unixpath.map(Result::Ok).unwrap_or_else(|| bail!("no unixpath provided")).context(Error::InvalidConfiguration)?;
		let ttl_config = self.ttl_config.unwrap_or_default();
		let unix_workers = self.unix_workers.unwrap_or(0);

		info!("[http-unix][run] parameters parsed");
		Unix
		{
			remote,
			domain,
			hostmaster,
			ttl_config,
			unixpath,
			unix_workers,
		}.run().await?;
		Ok(())
	}
}

