use crate::
{
	error::*,
};

use ::
{
	async_std::
	{
		os::unix::net::UnixListener,
		fs::remove_file,
		path::PathBuf,
	},
	futures::
	{
		TryStreamExt,
		StreamExt,
	},
};

pub struct Unix
{
	remote: Vec<String>,
	domain: String,
	hostmaster: String,
	unixpath: PathBuf,
	unix_workers: usize,
}

impl Unix
{
	pub fn builder() -> UnixBuilder
	{
		Default::default()
	}

	#[actix_web::main]
	pub async fn run(self) -> Result<()>
	{
		debug!("[http-unix] started");

		let path = self.unixpath.as_path();
		if path.exists().await
		{
			warn!("[http-unix] removing potentially stale socket");
			remove_file(path).await?;
		}

		let listener = UnixListener::bind(path).await?;
		info!("[http-unix] unix socket opened");

		listener.incoming().map(|res| res.context(Error::UnixServerError)).try_for_each_concurrent(self.unix_workers, |stream|
		{
			let me = &self;
			async move
			{
				debug!("[http-unix] connection opened");

				let backend = super::query::RemoteQuery::new(me.remote.clone()).await?;
				let handler = crate::pdns_io::PdnsStreamHandler::new(&me.domain, &me.hostmaster, backend, stream.clone(), stream).await?;
				handler.run().await?;

				debug!("[http-unix] connection closed");
				Ok(())
			}
		}).await?;

		remove_file(path).await?;
		debug!("[http-unix] stopped");

		Ok(())
	}
}

#[derive(Clone,Eq,PartialEq,Hash,Debug,Default)]
pub struct UnixBuilder
{
	remote: Option<Vec<String>>,
	domain: Option<String>,
	hostmaster: Option<String>,
	unixpath: Option<PathBuf>,
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
		self.unixpath = Some(PathBuf::from(unixpath));
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
		let unix_workers = self.unix_workers.unwrap_or(0);

		async_std::task::spawn_blocking(move || -> Result<()>
		{
			info!("[http-unix][run] parameters parsed");
			Unix
			{
				remote,
				domain,
				hostmaster,
				unixpath,
				unix_workers,
			}.run()?;

			Ok(())
		}).await?;

		Ok(())
	}
}

