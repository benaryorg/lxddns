pub mod error;
pub mod lxd;
pub mod pdns;

use crate::
{
	error::*,
	lxd::
	{
		AddressFamily,
		AddressScope,
		ContainerState,
		ContainerName,
	},
	pdns::
	{
		LookupType,
		Query,
		ResponseEntry,
	},
};

use ::
{
	lapin::
	{
		Connection,
		ExchangeKind,
		Channel,
		options::
		{
			BasicAckOptions,
			BasicConsumeOptions,
			BasicRejectOptions,
			QueueDeclareOptions,
		},
		protocol::basic::AMQPProperties,
	},
	log::
	{
		error,
		warn,
		info,
		debug,
		trace,
	},
	futures::
	{
		stream::
		{
			StreamExt,
			TryStreamExt,
		},
		future,
	},
	regex_static::static_regex,
	serde::
	{
		Deserialize,
		Deserializer,
		Serialize,
	},
	serde_json::
	{
		json,
	},
	uuid::Uuid,
	getset::Getters,
	async_std::
	{
		prelude::*,
		task,
		os::unix::net::UnixListener,
		io::
		{
			BufReader,
		},
		process::
		{
			Command,
			Stdio,
		},
		path::Path,
	},
	std::
	{
		convert::TryInto,
		time::
		{
			Duration,
			Instant,
		},
		str::FromStr,
		path::PathBuf,
		sync::Arc,
		collections::
		{
			HashMap,
		},
		net::Ipv6Addr,
	},
};

pub async fn remote_query(channel: &Channel, name: &ContainerName) -> Result<Option<Vec<Ipv6Addr>>>
{
	debug!("[remote_query][{}] starting query", name.as_ref());

	let queue = channel.queue_declare(
		"",
		QueueDeclareOptions
		{
			exclusive: true,
			auto_delete: true,
			..QueueDeclareOptions::default()
		},
		Default::default()
	).await.with_context(|| "error in queue_declare")?;

	let mut consumer = channel.basic_consume(queue.name().as_str(),
		"",
		BasicConsumeOptions
		{
			no_ack: false,
			no_local: true,
			..Default::default()
		},
		Default::default()
	).await.with_context(|| "error in basic_consume")?;

	let correlation_id = format!("{}", Uuid::new_v4());
	trace!("[remote_query][{}] correlation id: {}", name.as_ref(), correlation_id);

	channel.basic_publish("lxddns","lxddns",Default::default(),name.as_ref().as_bytes().to_vec(),
		AMQPProperties::default()
			.with_correlation_id(correlation_id.clone().into())
			.with_reply_to(queue.name().clone())
	).await.with_context(|| "error in basic_publish")?;
	trace!("[remote_query][{}][{}] published message", name.as_ref(), correlation_id);

	let mut result = None;

	// FIXME: this timeout needs to be configurable
	//  the timeout strongly depends on the latency between hosts, in my case ~250ms at most
	let mut timeout = Duration::from_millis(1000);
	let extension = Duration::from_millis(250);
	let instant = Instant::now();

	while let Ok(Some(Ok((_,delivery)))) = consumer.next().timeout(timeout.saturating_sub(instant.elapsed())).await
	{
		if delivery.properties.correlation_id().as_ref().map_or(false,|corr_id| corr_id.as_str().eq(&correlation_id))
		{
			let elapsed = instant.elapsed();
			trace!("[remote_query][{}][{}] got response after {:.3}s", name.as_ref(), correlation_id, elapsed.as_secs_f64());
			timeout = elapsed + (elapsed + 2*extension)/2;

			if let Ok(addresses) = delivery.data.chunks(16)
				.map(|v| Ok(Ipv6Addr::from(u128::from_le_bytes(v.to_vec().try_into()?))))
				.collect::<std::result::Result<Vec<_>,Vec<_>>>()
			{
				debug!("[remote_query][{}][{}] got response after {:.3}s: {:?}", name.as_ref(), correlation_id, instant.elapsed().as_secs_f64(), addresses);
				result.get_or_insert_with(Vec::new).extend(addresses);
				delivery.acker.ack(BasicAckOptions
				{
					multiple: false,
				}).await?;
			}
			else
			{
				debug!("[remote_query][{}][{}] invalid content; rejecting", name.as_ref(), correlation_id);
				delivery.acker.reject(BasicRejectOptions
				{
					requeue: false,
				}).await?;
			}
		}
		else
		{
			debug!("[remote_query][{}][{}] unrelated message received", name.as_ref(), correlation_id);
			delivery.acker.reject(BasicRejectOptions
			{
				requeue: true,
			}).await?;
		}
	}

	Ok(result)
}

pub async fn local_query(name: &ContainerName) -> Result<Option<Vec<Ipv6Addr>>>
{
	trace!("[local_query][{}] starting query", name.as_ref());

	let instant = Instant::now();

	// maybe switch to reqwest some day?

	trace!("[local_query][{}] getting instance list", name.as_ref());
	// first get the list of instances
	let output = Command::new("sudo")
		.arg("lxc")
		.arg("query")
		.arg("--")
		.arg("/1.0/instances")
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.output()
		.await
		.context(Error::LocalExecution(None))?;

	debug!("[local_query][{}] instance listing ran for {:.3}s", name.as_ref(), instant.elapsed().as_secs_f64());

	trace!("[local_query][{}] validating instance list command output", name.as_ref());
	if !output.status.success()
	{
		let err = String::from_utf8_lossy(&output.stderr);
		bail!(Error::LocalExecution(Some(err.to_string())))
	}

	trace!("[local_query][{}] parsing instance list", name.as_ref());
	let instances: Vec<String> = serde_json::from_slice(&output.stdout).context(Error::LocalOutput)?;

	trace!("[local_query][{}] validating and filtering instance list", name.as_ref());
	let instances = instances.into_iter()
		.filter_map(|instance|
		{
			let instance = match instance.strip_prefix("/1.0/instances/")
			{
				Some(instance) => instance,
				None => return None,
			};

			if name.as_ref().eq(instance)
			{
				trace!("[local_query][{}] exact match", name.as_ref());
				Some((true,instance.to_string()))
			}
			else
			{
				if let Some(remainder) = instance.strip_prefix(name.as_ref())
				{
					if !remainder.contains(|ch: char| !ch.is_ascii_digit())
					{
						trace!("[local_query][{}] prefix match: {}", name.as_ref(), instance);
						Some((false,instance.to_string()))
					}
					else
					{
						trace!("[local_query][{}] prefix does not match: {}", name.as_ref(), instance);
						None
					}
				}
				else
				{
					trace!("[local_query][{}] no match", name.as_ref());
					None
				}
			}
		})
		.collect::<Vec<_>>()
	;

	// this assumes that all matches are either exact or there is only one local instance matching
	// in all cases there will only be one query
	let instance = if let Some((_,instance)) = instances.iter().find(|(exact,_)| *exact)
	{
		Some(instance)
	}
	else
	{
		instances.get(0).map(|(_,instance)| instance)
	};

	let instance = match instance
	{
		Some(instance) =>
		{
			debug!("[local_query][{}] match: {}", name.as_ref(), instance);
			instance
		}
		None =>
		{
			debug!("[local_query][{}] not found", name.as_ref());
			return Ok(None);
		}
	};

	trace!("[local_query][{}] querying state", name.as_ref());
	let output = Command::new("sudo")
		.arg("lxc")
		.arg("query")
		.arg("--")
		.arg(format!("/1.0/instances/{}/state", instance))
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.output()
		.await
		.context(Error::LocalExecution(None))?;

	debug!("[local_query][{}] query ran for {:.3}s", name.as_ref(), instant.elapsed().as_secs_f64());

	if !output.status.success()
	{
		if &output.stderr == b"Error: not found\n"
		{
			trace!("[local_query][{}] \"not found\"", name.as_ref());
			return Ok(None);
		}
		let err = String::from_utf8_lossy(&output.stderr);
		bail!(Error::LocalExecution(Some(err.to_string())))
	}

	trace!("[local_query][{}] got response", name.as_ref());
	let state: ContainerState = serde_json::from_slice(&output.stdout).context(Error::LocalOutput)?;

	if state.status() != "Running"
	{
		trace!("[local_query][{}] not running", name.as_ref());
		return Ok(None);
	}

	let addresses = state.network()
		.values()
		.flat_map(|net| net.addresses().iter())
		.filter(|address| address.scope() == &AddressScope::Global && address.family() == &AddressFamily::Inet6)
		.filter_map(|address| address.address().parse::<Ipv6Addr>().ok())
		.collect::<Vec<_>>();

	trace!("[local_query][{}] result: {:?}", name.as_ref(), addresses);

	Ok(Some(addresses))
}

pub struct Server
{
	unixpath: PathBuf,
	domain: String,
	hostmaster: String,
	connection: Connection,
}

impl Server
{
	pub fn builder() -> ServerBuilder
	{
		Default::default()
	}

	pub async fn run(self) -> Result<()>
	{
		let me = Arc::new(self);

		let (responder, responder_abort) = future::abortable(task::spawn_local(me.clone().responder()));
		info!("[server][run] responder spawned");

		let listener = UnixListener::bind(me.unixpath.as_path()).await?;
		info!("[server][run] unix socket opened");

		let (unixserver, unixserver_abort) = future::abortable(task::spawn_local(me.clone().unixserver(listener)));
		info!("[server][run] unixserver started");

		info!("[server][run] running");
		match future::select(unixserver, responder).await
		{
			future::Either::Left((Ok(res), responder)) =>
			{
				responder_abort.abort();
				let _ = responder.await;
				let _ = res.context(Error::UnixServerError)?;
				bail!(Error::UnixServerClosed);
			},
			future::Either::Right((Ok(res), unixserver)) =>
			{
				unixserver_abort.abort();
				let _ = unixserver.await;
				let _ = res.context(Error::ResponderError)?;
				bail!(Error::ResponderClosed);
			},
			_ => unreachable!(),
		}
	}

	async fn responder(self: Arc<Self>) -> Result<()>
	{
		let channel = self.connection.create_channel().await.context(Error::QueueConnectionError)?;

		channel.exchange_declare("lxddns", ExchangeKind::Fanout, Default::default(), Default::default()).await?;
		trace!("[responder] created fanout exchange");

		let queue = channel.queue_declare(
			"",
			QueueDeclareOptions
			{
				exclusive: true,
				auto_delete: true,
				..QueueDeclareOptions::default()
			},
			Default::default()
		).await?;
		trace!("[responder] created queue");

		channel.queue_bind(queue.name().as_str(), "lxddns", "lxddns", Default::default(), Default::default()).await?;
		trace!("[responder] bound exchange to queue");

		// Start a consumer.
		let consumer = channel.basic_consume(queue.name().as_str(),
			"",
			BasicConsumeOptions
			{
				no_ack: false,
				no_local: true,
				..Default::default()
			},
			Default::default()
		).await?;
		info!("[responder] running");

		consumer.try_for_each_concurrent(10, |query|
		{
			let channel = &channel;

			async move
			{
				let (_,delivery) = query;

				debug!("[responder] received message");
				let name = String::from_utf8_lossy(&delivery.data);
				debug!("[responder][{}] received request", name);

				let name = match name.parse::<ContainerName>()
				{
					Ok(ok) => ok,
					Err(_) =>
					{
						info!("[responder][{}] invalid name; rejecting", name);
						delivery.acker.reject(BasicRejectOptions
						{
							requeue: false,
						}).await?;
						return Ok(());
					},
				};

				let (reply_to, corr_id) = match (delivery.properties.reply_to(),delivery.properties.correlation_id())
				{
					(Some(reply_to),Some(corr_id)) => (reply_to,corr_id),
					_ =>
					{
						info!("[responder][{}] message without reply_to or correlation_id; rejecting", name.as_ref());
						delivery.acker.reject(BasicRejectOptions
						{
							requeue: false,
						}).await?;
						return Ok(());
					}
				};

				let addresses = match local_query(&name).await
				{
					Ok(Some(addresses)) =>
					{
						trace!("[responder][{}] got {:?}", name.as_ref(), addresses);

						addresses
					},
					Ok(_) =>
					{
						trace!("[responder][{}] no info; rejecting", name.as_ref());
						delivery.acker.reject(BasicRejectOptions
						{
							requeue: false,
						}).await?;
						return Ok(());
					},
					Err(err) =>
					{
						warn!("[responder][{}] query error: {}", name.as_ref(), err);
						for err in err.chain().skip(1)
						{
							warn!("[responder][{}]  caused by: {}", name.as_ref(), err);
						}
						delivery.acker.reject(BasicRejectOptions
						{
							requeue: true,
						}).await?;
						return Ok(());
					},
				};
				let response = addresses.into_iter().flat_map(|addr| u128::from(addr).to_le_bytes().to_vec()).collect::<Vec<u8>>();

				channel.basic_publish("",reply_to.as_str(),Default::default(),response,
					AMQPProperties::default()
						.with_correlation_id(corr_id.clone())
				).await?;
				trace!("[responder][{}] message published", name.as_ref());
				delivery.acker.ack(BasicAckOptions
				{
					multiple: false,
				}).await?;

				Ok(())
			}
		}).await?;

		Ok(())
	}

	async fn unixserver(self: Arc<Self>, listener: UnixListener) -> Result<()>
	{
		let soa_record = &ResponseEntry::soa(&self.domain, &self.hostmaster);

		listener.incoming().map(|res| res.context(Error::UnixServerError)).try_for_each_concurrent(10, |stream|
		{
			debug!("[unixserver] connection opened");
			let mut writer = stream;
			let reader = BufReader::new(writer.clone());

			let me = self.clone();

			trace!("[unix_server] starting async task");
			async move
			{
				trace!("[unixserver] async task running");

				let channel = me.connection.create_channel().await.context(Error::QueueConnectionError)?;
				trace!("[unixserver] channel created");

				let mut lines = reader.split(b'\n');
				while let Some(input) = lines.next().await
				{
					trace!("[unixserver] request received");
					let input = match input
					{
						Err(err) =>
						{
							warn!("[unixserver] read error: {}", err);
							break;
						},
						Ok(ok) => ok,
					};

					trace!("[unixserver] parsing request");
					match serde_json::from_slice::<Query>(&input)
					{
						Ok(Query::Lookup { parameters: query, }) =>
						{
							debug!("[unixserver][{}] type {}", query.qname(), query.qtype());

							match query.type_for_domain(&me.domain)
							{
								LookupType::Smart { container, response, } =>
								{
									debug!("[unixserver][{}] smart response, querying {}", query.qname(), container.as_ref());

									let instant = Instant::now();
									let result = remote_query(&channel,&container).timeout(Duration::from_millis(4500)).await;

									debug!("[unixserver][{}] remote_query ran for {:.3}s (timeout: {})", query.qname(), instant.elapsed().as_secs_f64(), result.is_err());

									match result.ok().unwrap_or(Ok(None))
									{
										Ok(result) =>
										{
											debug!("[unixserver][{}] got {:?}", query.qname(), result);

											let response = response.response(query.qname(), soa_record, result);

											match serde_json::to_string(&response)
											{
												Ok(json) =>
												{
													if let Err(err) = writeln!(writer, "{}", json).await
													{
														warn!("[unixserver][{}] closing unix stream due to socket error: {}", query.qname(), err);
														break;
													}
												},
												Err(err) =>
												{
													warn!("[unixserver][{}] closing unix stream due to json error: {}", query.qname(), err);
													break;
												},
											}
										},
										Err(err) =>
										{
											error!("[unixserver][{}] resolve error, assuming taint: {}", query.qname(), err);
											Err(err).context(Error::MessageQueueChannelTaint)?;
										},
									}
								},
								LookupType::Dumb { response, } =>
								{
									debug!("[unixserver][{}] dumb response", query.qname());

									let response = response.response(query.qname(), soa_record);

									match serde_json::to_string(&response)
									{
										Ok(json) =>
										{
											if let Err(err) = writeln!(writer, "{}", json).await
											{
												warn!("[unixserver][{}] closing unix stream due to socket error: {}", query.qname(), err);
												break;
											}
										},
										Err(err) =>
										{
											warn!("[unixserver][{}] closing unix stream due to json error: {}", query.qname(), err);
											break;
										},
									}
								},
							}
						},
						Ok(Query::Initialize) =>
						{
							if let Err(err) = writeln!(writer, "{}", json!({ "result": true }).to_string()).await
							{
								warn!("[unixserver] closing unix stream due to socket error: {}", err);
								break;
							}
						},
						Ok(Query::Unknown) =>
						{
							debug!("[unixserver] unknown query: {:?}", String::from_utf8_lossy(&input));
							if let Err(err) = writeln!(writer, "{}", json!({ "result": false }).to_string()).await
							{
								warn!("[unixserver] closing unix stream due to socket error: {}", err);
								break;
							}
						},
						Err(err) =>
						{
							warn!("[unixserver] error parsing request: {}", err);
							break;
						},
					}
				}
				debug!("[unixserver] connection closed");
				Ok(())
			}
		}).await?;
		Ok(())
	}
}

#[derive(Clone,Eq,PartialEq,Hash,Debug,Default)]
pub struct ServerBuilder
{
	unixpath: Option<PathBuf>,
	url: Option<String>,
	domain: Option<String>,
	hostmaster: Option<String>,
}

impl ServerBuilder
{
	pub fn unixpath<P: AsRef<Path>>(mut self, path: P) -> Self
	{
		self.unixpath = Some(path.as_ref().into());
		return self;
	}

	pub fn url<S: AsRef<str>>(mut self, url: S) -> Self
	{
		self.url = Some(url.as_ref().into());
		return self;
	}

	pub fn domain<S: AsRef<str>>(mut self, domain: S) -> Self
	{
		self.domain = Some(domain.as_ref().into());
		return self;
	}

	pub fn hostmaster<S: AsRef<str>>(mut self, hostmaster: S) -> Self
	{
		self.hostmaster = Some(hostmaster.as_ref().into());
		return self;
	}

	pub async fn run(self) -> Result<()>
	{
		let unixpath = self.unixpath.map(Result::Ok).unwrap_or_else(|| bail!("no unixpath provided")).context(Error::InvalidConfiguration)?;
		let url = self.url.map(Result::Ok).unwrap_or_else(|| bail!("no url provided")).context(Error::InvalidConfiguration)?;
		let domain = self.domain.map(Result::Ok).unwrap_or_else(|| bail!("no domain provided")).context(Error::InvalidConfiguration)?;
		let hostmaster = self.hostmaster.map(Result::Ok).unwrap_or_else(|| bail!("no hostmaster provided")).context(Error::InvalidConfiguration)?;

		let connection = Connection::connect(url.as_ref(), Default::default())
			.await
			.context("connect failed")
			.context(Error::QueueConnectionError)
		?;
		info!("[server][run] connection to message queue established");

		let server = Server { unixpath, domain, hostmaster, connection, };

		server.run().await
	}
}

