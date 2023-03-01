use crate::
{
	error::*,
};

use ::
{
	lapin::
	{
		Connection,
	},
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
	domain: String,
	hostmaster: String,
	connection: Connection,
	unixpath: String,
	unix_workers: usize,
}

impl Unix
{
	pub fn builder() -> UnixBuilder
	{
		Default::default()
	}

	pub async fn run(self) -> Result<()>
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
		info!("[unix] unix socket opened");

		UnixListenerStream::new(listener).map(|res| res.context(Error::UnixServerError)).try_for_each_concurrent(self.unix_workers, |stream|
		{
			let me = &self;
			async move
			{
				debug!("[unix] connection opened");

				let channel = me.connection.create_channel().await?;
				debug!("[unix] channel created");

				let backend = super::query::RemoteQuery::new(channel).await?;
				let (read, write) = stream.into_split();
				let handler = crate::pdns_io::PdnsStreamHandler::new(&me.domain, &me.hostmaster, backend, read, write).await?;
				handler.run().await?;

				debug!("[unix] connection closed");
				Ok(())
			}
		}).await?;

		remove_file(self.unixpath).await?;
		debug!("[unix] stopped");

		Ok(())
	}
}

#[derive(Clone,Eq,PartialEq,Hash,Debug,Default)]
pub struct UnixBuilder
{
	url: Option<String>,
	domain: Option<String>,
	hostmaster: Option<String>,
	unixpath: Option<String>,
	unix_workers: Option<usize>,
}

impl UnixBuilder
{
	pub fn url<S: AsRef<str>>(mut self, url: S) -> Self
	{
		self.url = Some(url.as_ref().into());
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

	pub fn unix_workers(mut self, unix_workers: usize) -> Self
	{
		self.unix_workers = Some(unix_workers);
		self
	}

	pub async fn run(self) -> Result<()>
	{
		let url = self.url.map(Result::Ok).unwrap_or_else(|| bail!("no url provided")).context(Error::InvalidConfiguration)?;
		let domain = self.domain.map(Result::Ok).unwrap_or_else(|| bail!("no domain provided")).context(Error::InvalidConfiguration)?;
		let hostmaster = self.hostmaster.map(Result::Ok).unwrap_or_else(|| bail!("no hostmaster provided")).context(Error::InvalidConfiguration)?;
		let unixpath = self.unixpath.map(Result::Ok).unwrap_or_else(|| bail!("no unixpath provided")).context(Error::InvalidConfiguration)?;
		let unix_workers = self.unix_workers.unwrap_or(0);

		let connection = Connection::connect(url.as_ref(), Default::default())
			.await
			.context("connect failed")
			.context(Error::QueueConnectionError)
		?;

		Unix
		{
			domain,
			hostmaster,
			connection,
			unixpath,
			unix_workers,
		}.run().await
	}
}

