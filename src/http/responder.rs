// Copyright (C) benaryorg <binary@benary.org>
//
// This software is licensed as described in the file COPYING, which
// you should have received as part of this distribution.
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::
{
	error::*,
	lxd::
	{
		local_query,
	},
	http::
	{
		ApiResponse,
		ApiResponseV1,
	},
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
		HttpRequest,
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
async fn resolve(request: HttpRequest, name: web::Path<String>) -> impl ActixResponder
{
	trace!("[http-responder][resolve] request received for {:?}", name);

	let config: Result<&web::Data<ResponderConfig>> = request.app_data().ok_or(Error::ResponderError).context("cannot retrieve app_data");

	match config.and_then(|config| Ok((config, name.parse()?)))
	{
		Err(err) => HttpResponse::BadRequest().body(format!("{}", err)),
		Ok((config, name)) => match local_query(&config.command, &name).await
		{
			Ok(None) => HttpResponse::Ok().json(ApiResponse::V1(ApiResponseV1::NoMatch)),
			Ok(Some(res)) => HttpResponse::Ok().json(ApiResponse::V1(ApiResponseV1::AnyMatch(res))),
			Err(err) => HttpResponse::InternalServerError().body(format!("{}", err)),
		},
	}
}

#[derive(Clone)]
pub struct ResponderConfig
{
	command: String,
}

pub struct Responder
{
	config: ResponderConfig,
	https_bind: String,
	tls_config: ServerConfig,
}

impl Responder
{
	pub fn builder() -> ResponderBuilder
	{
		Default::default()
	}

	pub async fn run(self) -> Result<()>
	{
		info!("[http-responder][run] webserver starting");

		let config = self.config;

		HttpServer::new(move ||
			{
				App::new()
					.app_data(web::Data::new(config.clone()))
					.service(resolve)
			})
			.bind_rustls_021(self.https_bind, self.tls_config)?
			.run()
			.await?;

		info!("[http-responder][run] webserver stopped");

		Ok(())
	}
}

#[derive(Clone,Eq,PartialEq,Hash,Debug,Default)]
pub struct ResponderBuilder
{
	command: Option<String>,
	https_bind: Option<String>,
	tls_key: Option<String>,
	tls_chain: Option<String>,
}

impl ResponderBuilder
{
	pub fn command<S: AsRef<str>>(mut self, command: S) -> Self
	{
		self.command = Some(command.as_ref().into());
		self
	}

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
		let command = self.command.map(Result::Ok).unwrap_or_else(|| bail!("no command provided")).context(Error::InvalidConfiguration)?;
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
			.map(|res| Ok(Certificate(res?.to_vec())))
			.collect::<Result<_>>()
			.context("cannot load certificate chain")?;

		let tls_key = read_all(tls_key)
			.map(|res| Ok(res?))
			.collect::<Result<Vec<_>>>()
			.context("cannot load private key")?
			.into_iter()
			.filter_map(|item| match item
			{
				Item::Pkcs1Key(key) => Some(PrivateKey(key.secret_pkcs1_der().into())),
				Item::Pkcs8Key(key) => Some(PrivateKey(key.secret_pkcs8_der().into())),
				Item::Sec1Key(key) => Some(PrivateKey(key.secret_sec1_der().into())),
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

		Responder
		{
			config: ResponderConfig
			{
				command,
			},
			tls_config,
			https_bind,
		}.run().await?;
		Ok(())
	}
}

