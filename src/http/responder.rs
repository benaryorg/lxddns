use crate::
{
	error::*,
	lxd::
	{
		local_query,
	},
	http::ApiResponse,
};

use ::
{
	log::
	{
		info,
	},
	actix_web::
	{
		get,
		web,
		App,
		HttpServer,
		Responder as ActixResponder,
		HttpResponse,
	},
	rustls::
	{
		Certificate,
		PrivateKey,
		ServerConfig,
	},
	rustls_pemfile::
	{
		read_all,
		certs,
		Item,
	},
	std::
	{
		fs::File,
		io::BufReader,
	},
};

#[get("/resolve/v1/{name}")]
async fn resolve(name: web::Path<String>) -> impl ActixResponder
{
	trace!("[http-responder][resolve] request received for {:?}", name);

	match name.parse()
	{
		Err(err) => HttpResponse::BadRequest().body(format!("{}", err)),
		Ok(name) => match local_query(&name).await
		{
			Ok(res) => HttpResponse::Ok().json(ApiResponse::V1(res)),
			Err(err) => HttpResponse::InternalServerError().body(format!("{}", err)),
		},
	}
}

pub struct Responder
{
	https_bind: String,
	tls_config: ServerConfig,
}

impl Responder
{
	pub fn builder() -> ResponderBuilder
	{
		Default::default()
	}

	#[actix_web::main]
	pub async fn run(self) -> Result<()>
	{
		info!("[http-responder][run] webserver starting");

		HttpServer::new(||
			{
				App::new()
					.service(resolve)
			})
			.bind_rustls(self.https_bind, self.tls_config)?
			.run()
			.await?;

		info!("[http-responder][run] webserver stopped");

		Ok(())
	}
}

#[derive(Clone,Eq,PartialEq,Hash,Debug,Default)]
pub struct ResponderBuilder
{
	https_bind: Option<String>,
	tls_key: Option<String>,
	tls_chain: Option<String>,
}

impl ResponderBuilder
{
	pub fn https_bind<S: AsRef<str>>(mut self, https_bind: S) -> Self
	{
		self.https_bind = Some(https_bind.as_ref().into());
		self
	}

	pub fn tls_key<S: AsRef<str>>(mut self, tls_key: S) -> Self
	{
		self.tls_key = Some(tls_key.as_ref().into());
		self
	}

	pub fn tls_chain<S: AsRef<str>>(mut self, tls_chain: S) -> Self
	{
		self.tls_chain = Some(tls_chain.as_ref().into());
		self
	}

	pub async fn run(self) -> Result<()>
	{
		let https_bind = self.https_bind.map(Result::Ok).unwrap_or_else(|| bail!("no https_bind provided")).context(Error::InvalidConfiguration)?;
		let tls_key = self.tls_key.map(Result::Ok).unwrap_or_else(|| bail!("no tls_key provided")).context(Error::InvalidConfiguration)?;
		let tls_chain = self.tls_chain.map(Result::Ok).unwrap_or_else(|| bail!("no tls_chain provided")).context(Error::InvalidConfiguration)?;

		let tls_config = ServerConfig::builder()
			.with_safe_defaults()
			.with_no_client_auth();

		let tls_chain = &mut BufReader::new(File::open(tls_chain).unwrap());
		let tls_key = &mut BufReader::new(File::open(tls_key).unwrap());

		// convert files to key/cert objects
		let tls_chain = certs(tls_chain)
			.unwrap()
			.into_iter()
			.map(Certificate)
			.collect();

		let tls_key = read_all(tls_key)
			.unwrap()
			.into_iter()
			.filter_map(|item| match item
			{
				Item::RSAKey(key) => Some(PrivateKey(key)),
				Item::PKCS8Key(key) => Some(PrivateKey(key)),
				Item::ECKey(key) => Some(PrivateKey(key)),
				_ => None,
			})
			.next()
			.map(Result::Ok)
			.unwrap_or_else(|| bail!("no tls key found"))
			.context(Error::InvalidConfiguration)?
		;

		let tls_config = tls_config.with_single_cert(tls_chain, tls_key)
			.with_context(|| "tls configuration invalid")
			.context(Error::InvalidConfiguration)?
		;

		info!("[http-responder][run] certificates parsed");

		async_std::task::spawn_blocking(move || -> Result<()>
		{
			info!("[http-responder][run] parameters parsed");
			Responder
			{
				tls_config,
				https_bind,
			}.run()?;

			Ok(())
		}).await?;

		Ok(())
	}
}

