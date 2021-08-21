pub mod error;
pub mod lxd;
pub mod pdns;

use self::
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
			QueueDeclareOptions,
			BasicConsumeOptions,
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
	error_chain::*,
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
	channel.exchange_declare("lxddns", ExchangeKind::Fanout, Default::default(), Default::default()).await?;
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
	let mut consumer = channel.basic_consume(queue.name().as_str(),
		"",
		BasicConsumeOptions
		{
			no_ack: true,
			no_local: true,
			..Default::default()
		},
		Default::default()
	).await?;

	let correlation_id = format!("{}", Uuid::new_v4());
	trace!("[remote_query][{}] correlation id: {}", name.as_ref(), correlation_id);

	channel.basic_publish("lxddns","lxddns",Default::default(),name.as_ref().as_bytes().to_vec(),
		AMQPProperties::default()
			.with_correlation_id(correlation_id.clone().into())
			.with_reply_to(queue.name().clone())
	).await?;
	trace!("[remote_query][{}][{}] published message", name.as_ref(), correlation_id);

	let mut result = None;

	// FIXME: this timeout needs to be configurable
	//  the timeout strongly depends on the latency between hosts, in my case ~250ms at most
	let timeout = Duration::from_millis(300);
	let instant = Instant::now();

	while let Ok(Some(Ok((_,delivery)))) = consumer.next().timeout(timeout.saturating_sub(instant.elapsed())).await
	{
		trace!("[remote_query][{}][{}] got response", name.as_ref(), correlation_id);

		if delivery.properties.correlation_id().as_ref().map_or(false,|corr_id| corr_id.as_str().eq(&correlation_id))
		{
			if let Ok(addresses) = delivery.data.chunks(16)
				.map(|v| Ok(Ipv6Addr::from(u128::from_le_bytes(v.to_vec().try_into()?))))
				.collect::<std::result::Result<Vec<_>,Vec<_>>>()
			{
				debug!("[remote_query][{}][{}] got response after {:.3}s: {:?}", name.as_ref(), correlation_id, instant.elapsed().as_secs_f64(), addresses);
				result.get_or_insert_with(|| Vec::new()).extend(addresses);
			}
			else
			{
				debug!("[remote_query][{}][{}] invalid content", name.as_ref(), correlation_id);
			}
		}
		else
		{
			debug!("[remote_query][{}][{}] missing reply_to or correlation_id", name.as_ref(), correlation_id);
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
		.chain_err(|| ErrorKind::LocalExecution(None))?;

	debug!("[local_query][{}] instance listing ran for {:.3}s", name.as_ref(), instant.elapsed().as_secs_f64());

	trace!("[local_query][{}] validating instance list command output", name.as_ref());
	if !output.status.success()
	{
		let err = String::from_utf8_lossy(&output.stderr);
		bail!(ErrorKind::LocalExecution(Some(err.to_string())))
	}

	trace!("[local_query][{}] parsing instance list", name.as_ref());
	let instances: Vec<String> = serde_json::from_slice(&output.stdout).chain_err(|| ErrorKind::LocalOutput)?;

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
		.chain_err(|| ErrorKind::LocalExecution(None))?;

	debug!("[local_query][{}] query ran for {:.3}s", name.as_ref(), instant.elapsed().as_secs_f64());

	if !output.status.success()
	{
		if &output.stderr == b"Error: not found\n"
		{
			trace!("[local_query][{}] \"not found\"", name.as_ref());
			return Ok(None);
		}
		let err = String::from_utf8_lossy(&output.stderr);
		bail!(ErrorKind::LocalExecution(Some(err.to_string())))
	}

	trace!("[local_query][{}] got response", name.as_ref());
	let state: ContainerState = serde_json::from_slice(&output.stdout).chain_err(|| ErrorKind::LocalOutput)?;

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

pub async fn responder(channel: Channel) -> Result<()>
{
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
	let mut consumer = channel.basic_consume(queue.name().as_str(),
		"",
		BasicConsumeOptions
		{
			no_ack: true,
			no_local: true,
			..Default::default()
		},
		Default::default()
	).await?;
	info!("[responder] running");

	while let Some(Ok((_,delivery))) = consumer.next().await
	{
		debug!("[responder] received message");
		let name = String::from_utf8_lossy(&delivery.data);
		debug!("[responder][{}] received request", name);

		let name = match name.parse::<ContainerName>()
		{
			Ok(ok) => ok,
			Err(_) =>
			{
				info!("[responder][{}] invalid name", name);
				continue;
			},
		};

		let (reply_to, corr_id) = match (delivery.properties.reply_to(),delivery.properties.correlation_id())
		{
			(Some(reply_to),Some(corr_id)) => (reply_to,corr_id),
			_ =>
			{
				info!("[responder][{}] message without reply_to or correlation_id; acking and ignoring", name.as_ref());
				continue;
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
				trace!("[responder][{}] no info, skipping", name.as_ref());
				continue;
			},
			Err(err) =>
			{
				warn!("[responder][{}] query error: {}", name.as_ref(), err);
				continue;
			},
		};
		let response = addresses.into_iter().flat_map(|addr| u128::from(addr).to_le_bytes().to_vec()).collect::<Vec<u8>>();

		channel.basic_publish("",reply_to.as_str(),Default::default(),response,
			AMQPProperties::default()
				.with_correlation_id(corr_id.clone())
		).await?;
		trace!("[responder][{}] message published", name.as_ref());
	}

	Ok(())
}

pub async fn unixserver<S: AsRef<str>>(connection: Connection, listener: UnixListener, domain: S, hostmaster: S) -> Result<()>
{
	let soa_record = &ResponseEntry::soa(&domain, &hostmaster);

	listener.incoming().map(|res| res.map_err(|err| ErrorKind::UnixServerError(Box::new(err.into())))).try_for_each_concurrent(10, |stream|
	{
		debug!("[unixserver] connection opened");
		let mut writer = stream;
		let channel = connection.create_channel();
		let reader = BufReader::new(writer.clone());

		let domain = domain.as_ref().to_string();

		trace!("[unix_server] starting async task");
		async move
		{
			let channel = channel.await.chain_err(|| ErrorKind::MessageQueueChannelTaint)?;
			trace!("[unixserver] async task running");
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

						match query.type_for_domain(&domain)
						{
							LookupType::Smart { container, response, } =>
							{
								debug!("[unixserver][{}] smart response, querying {}", query.qname(), container.as_ref());

								match remote_query(&channel,&container).await
								{
									Ok(result) =>
									{
										debug!("[unixserver][{}] got {:?}", query.qname(), result);

										let response = response.response(query.qname(), &soa_record, result);

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
										Err(err).chain_err(|| ErrorKind::MessageQueueChannelTaint)?;
									},
								}
							},
							LookupType::Dumb { response, } =>
							{
								debug!("[unixserver][{}] dumb response", query.qname());

								let response = response.response(query.qname(), &soa_record);

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
						debug!("[unixserver] unknown method: {:?}", String::from_utf8_lossy(&input));
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

pub async fn run<S: AsRef<str>,P: AsRef<Path>>(unixpath: P, url: S, domain: S, hostmaster: S) -> Result<()> // use never when available
{
	let domain = domain.as_ref().to_string();
	let hostmaster = hostmaster.as_ref().to_string();

	let connection = Connection::connect(url.as_ref(),Default::default()).await?;
	info!("[run] connection to message queue established");

	let channel = connection.create_channel().await?;
	debug!("[run] channel created");
	let responder = task::spawn_local(async move { responder(channel).await });
	info!("[run] responder spawned");

	let listener = UnixListener::bind(unixpath.as_ref()).await?;
	info!("[run] unix socket opened");

	let unixserver = task::spawn_local(async move { unixserver(connection,listener,domain,hostmaster).await });
	info!("[run] unixserver started");

	info!("[run] running");
	match future::select(unixserver,responder).await
	{
		future::Either::Left((Ok(()), _)) => Err(ErrorKind::UnixServerClosed.into()),
		future::Either::Left((Err(err), _)) => Err(ErrorKind::UnixServerError(Box::new(err)).into()),
		future::Either::Right((Ok(()), _)) => Err(ErrorKind::ResponderClosed.into()),
		future::Either::Right((Err(err), _)) => Err(ErrorKind::ResponderError(Box::new(err)).into()),
	}
}

