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
		ContainerName,
	},
	pdns::
	{
		LookupType,
		Query,
		ResponseEntry,
		TtlConfig,
	},
};

use ::
{
	serde_json::
	{
		json,
	},
	tokio::
	{
		io::
		{
			BufReader,
			AsyncBufReadExt,
			AsyncRead,
			AsyncWrite,
			AsyncWriteExt,
		},
		time::timeout,
	},
	std::
	{
		time::
		{
			Duration,
			Instant,
		},
		net::Ipv6Addr,
	},
};

/// Trait specifying how to query a remote backend.
#[async_trait::async_trait]
pub trait RemoteQuery
{
	async fn remote_query(&self, name: &ContainerName) -> Result<Option<Vec<Ipv6Addr>>>;
	fn name(&self) -> String;
}

pub struct PdnsStreamHandler<R, W, B>
	where
		R: AsyncRead+Unpin,
		W: AsyncWrite+Unpin,
		B: RemoteQuery,
{
	domain: String,
	hostmaster: String,
	ttl_config: TtlConfig,
	backend: B,
	reader: R,
	writer: W,
}

impl<R, W, B> PdnsStreamHandler<R, W, B>
	where
		R: AsyncRead+Unpin,
		W: AsyncWrite+Unpin,
		B: RemoteQuery,
{
	pub async fn new<S1, S2>(domain: S1, ttl_config: &TtlConfig, hostmaster: S2, backend: B, reader: R, writer: W) -> Result<Self>
		where
			S1: AsRef<str>,
			S2: AsRef<str>,
	{
		Ok(Self
		{
			domain: domain.as_ref().to_string(),
			hostmaster: hostmaster.as_ref().to_string(),
			ttl_config: ttl_config.clone(),
			backend,
			reader,
			writer,
		})
	}

	pub async fn run(mut self) -> Result<()>
	{
		let soa_record = &ResponseEntry::soa(&self.domain, &self.ttl_config, &self.hostmaster);
		let ttl_config = &self.ttl_config;

		debug!("[pdns_io][handler] handling stream with queue {}", self.backend.name());

		let mut lines = BufReader::new(&mut self.reader).split(b'\n');
		while let Some(input) = lines.next_segment().await.transpose()
		{
			trace!("[pdns_io][handler] request received");
			let input = match input
			{
				Err(err) =>
				{
					warn!("[pdns_io][handler] read error: {}", err);
					break;
				},
				Ok(ok) => ok,
			};

			trace!("[pdns_io][handler] parsing request");
			match serde_json::from_slice::<Query>(&input)
			{
				Ok(Query::Lookup { parameters: query, }) =>
				{
					debug!("[pdns_io][handler][{}] type {}", query.qname(), query.qtype());

					match query.type_for_domain(&self.domain)
					{
						LookupType::Smart { container, response, } =>
						{
							debug!("[pdns_io][handler][{}] smart response, querying {}", query.qname(), container.as_ref());

							let instant = Instant::now();
							let result = timeout(Duration::from_millis(4500),self.backend.remote_query(&container)).await;

							debug!("[pdns_io][handler][{}] remote_query ran for {:.3}s (timeout: {})", query.qname(), instant.elapsed().as_secs_f64(), result.is_err());

							match result.ok().unwrap_or(Ok(None))
							{
								Ok(result) =>
								{
									debug!("[pdns_io][handler][{}] got {:?}", query.qname(), result);

									let response = response.response(query.qname(), ttl_config, soa_record, result);

									match serde_json::to_string(&response)
									{
										Ok(json) =>
										{
											if let Err(err) = self.writer.write_all(format!("{}\n", json).as_bytes()).await
											{
												warn!("[pdns_io][handler][{}] closing unix stream due to socket error: {}", query.qname(), err);
												break;
											}
										},
										Err(err) =>
										{
											warn!("[pdns_io][handler][{}] closing unix stream due to json error: {}", query.qname(), err);
											break;
										},
									}
								},
								Err(err) =>
								{
									warn!("[pdns_io][handler][{}] resolve error, assuming taint: {}", query.qname(), err);
									Err(err).context(Error::MessageQueueTaint)?;
								},
							}
						},
						LookupType::Dumb { response, } =>
						{
							debug!("[pdns_io][handler][{}] dumb response", query.qname());

							let response = response.response(query.qname(), ttl_config, soa_record);

							match serde_json::to_string(&response)
							{
								Ok(json) =>
								{
									if let Err(err) = self.writer.write_all(format!("{}\n", json).as_bytes()).await
									{
										warn!("[pdns_io][handler][{}] closing unix stream due to socket error: {}", query.qname(), err);
										break;
									}
								},
								Err(err) =>
								{
									warn!("[pdns_io][handler][{}] closing unix stream due to json error: {}", query.qname(), err);
									break;
								},
							}
						},
					}
				},
				Ok(Query::Initialize) =>
				{
					if let Err(err) = self.writer.write_all(format!("{}\n", json!({ "result": true })).as_bytes()).await
					{
						warn!("[pdns_io][handler] closing unix stream due to socket error: {}", err);
						break;
					}
				},
				Ok(Query::Unknown) =>
				{
					debug!("[pdns_io][handler] unknown query: {:?}", String::from_utf8_lossy(&input));
					if let Err(err) = self.writer.write_all(format!("{}\n", json!({ "result": false })).as_bytes()).await
					{
						warn!("[pdns_io][handler] closing unix stream due to socket error: {}", err);
						break;
					}
				},
				Err(err) =>
				{
					warn!("[pdns_io][handler] error parsing request: {}", err);
					break;
				},
			}

			trace!("[pdns_io][handler] flushing");
			if let Err(err) = self.writer.flush().await
			{
				warn!("[pdns_io][handler] closing unix stream due to socket error: {}", err);
				break;
			}
		}
		debug!("[pdns_io][handler] connection closed");

		Ok(())
	}
}

