use crate::
{
	error::*,
};

use ::
{
	async_std::
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
		let handler = crate::pdns_io::PdnsStreamHandler::new(self.domain, self.hostmaster, backend, stdin(), stdout()).await?;
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

	pub async fn run(self) -> Result<()>
	{
		let remote = self.remote.map(Result::Ok).unwrap_or_else(|| bail!("no remote provided")).context(Error::InvalidConfiguration)?;
		let domain = self.domain.map(Result::Ok).unwrap_or_else(|| bail!("no domain provided")).context(Error::InvalidConfiguration)?;
		let hostmaster = self.hostmaster.map(Result::Ok).unwrap_or_else(|| bail!("no hostmaster provided")).context(Error::InvalidConfiguration)?;

		Pipe
		{
			remote,
			domain,
			hostmaster,
		}.run().await
	}
}

