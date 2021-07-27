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
		Response,
		ResponseEntry,
	},
};

use ::
{
	clap::
	{
		app_from_crate,
		crate_description,
		crate_authors,
		crate_version,
		crate_name,
		Arg,
	},
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
		fs::remove_file,
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

async fn remote_query(channel: &Channel, name: &ContainerName) -> Result<Option<Vec<Ipv6Addr>>>
{
	debug!("starting remote query for {}", name.as_ref());
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
			no_ack: false,
			no_local: true,
			..Default::default()
		},
		Default::default()
	).await?;

	let correlation_id = format!("{}", Uuid::new_v4());
	trace!("query for {} has correlation id: {}", name.as_ref(), correlation_id);

	channel.basic_publish("lxddns","lxddns",Default::default(),name.as_ref().as_bytes().to_vec(),
		AMQPProperties::default()
			.with_correlation_id(correlation_id.clone().into())
			.with_reply_to(queue.name().clone())
	).await?;
	trace!("query for {} published the message", name.as_ref());

	while let Ok(Some(Ok((_,delivery)))) = consumer.next().timeout(Duration::from_millis(5000)).await
	{
		if delivery.properties.correlation_id().as_ref().map_or(false,|corr_id| corr_id.as_str().eq(&correlation_id))
		{
			if let Ok(addresses) = delivery.data.chunks(16)
				.map(|v| Ok(u128::from_le_bytes(v.to_vec().try_into()?).into()))
				.collect::<std::result::Result<Vec<_>,Vec<_>>>()
			{
				return Ok(Some(addresses));
			}
		}
		else
		{
			debug!("silently dropping message without either correlation_id or reply_to");
		}
	}
	Ok(None)
}

async fn local_query(name: &ContainerName) -> Result<Option<Vec<Ipv6Addr>>>
{
	trace!("starting local query for {}", name.as_ref());

	let instant = Instant::now();

	// maybe switch to reqwest some day?
	let output = Command::new("sudo")
		.arg("lxc")
		.arg("query")
		.arg("--")
		.arg(format!("/1.0/instances/{}/state", name.as_ref()))
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.output()
		.await
		.chain_err(|| ErrorKind::LocalExecution(None))?;

	debug!("local query ran for {:.3}s", instant.elapsed().as_secs_f64());

	if !output.status.success()
	{
		if &output.stderr == b"Error: not found\n"
		{
			trace!("local query got \"not found\" for {}",name.as_ref());
			return Ok(None);
		}
		let err = String::from_utf8_lossy(&output.stderr);
		bail!(ErrorKind::LocalExecution(Some(err.to_string())))
	}

	trace!("local query got response for {}",name.as_ref());
	let state: ContainerState = serde_json::from_slice(&output.stdout).chain_err(|| ErrorKind::LocalOutput)?;

	if state.status() != "Running"
	{
		trace!("local query got says {} is not running",name.as_ref());
		return Ok(None);
	}

	let addresses = state.network()
		.values()
		.flat_map(|net| net.addresses().iter())
		.filter(|address| address.scope() == &AddressScope::Global && address.family() == &AddressFamily::Inet6)
		.filter_map(|address| address.address().parse::<Ipv6Addr>().ok())
		.collect::<Vec<_>>();

	trace!("local query for {} yielded: {:?}", name.as_ref(), addresses);

	Ok(Some(addresses))
}

async fn responder(channel: Channel) -> Result<()>
{
	channel.exchange_declare("lxddns", ExchangeKind::Fanout, Default::default(), Default::default()).await?;
	trace!("created fanout exchange");

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
	trace!("created queue");

	channel.queue_bind(queue.name().as_str(), "lxddns", "lxddns", Default::default(), Default::default()).await?;
	trace!("bound exchange to queue");

	// Start a consumer.
	let mut consumer = channel.basic_consume(queue.name().as_str(),
		"",
		BasicConsumeOptions
		{
			no_ack: false,
			no_local: true,
			..Default::default()
		},
		Default::default()
	).await?;
	info!("responder online");

	while let Some(Ok((_,delivery))) = consumer.next().await
	{
		debug!("received message");
		let name = String::from_utf8_lossy(&delivery.data);
		debug!("request for {}", name);

		let name = match name.parse::<ContainerName>()
		{
			Ok(ok) => ok,
			Err(_) =>
			{
				info!("invalid name: {}", name);
				continue;
			},
		};

		let (reply_to, corr_id) = match (delivery.properties.reply_to(),delivery.properties.correlation_id())
		{
			(Some(reply_to),Some(corr_id)) => (reply_to,corr_id),
			_ =>
			{
				info!("received message without reply_to or correlation_id; acking and ignoring");
				continue;
			}
		};

		let addresses = match local_query(&name).await
		{
			Ok(Some(addresses)) => addresses,
			Ok(_) =>
			{
				trace!("no info on {}, skipping", name.as_ref());
				continue;
			},
			Err(err) =>
			{
				warn!("failure on getting local record: {}", err);
				continue;
			},
		};
		let response = addresses.into_iter().flat_map(|addr| u128::from(addr).to_le_bytes().to_vec()).collect::<Vec<u8>>();

		channel.basic_publish("",reply_to.as_str(),Default::default(),response,
			AMQPProperties::default()
				.with_correlation_id(corr_id.clone())
		).await?;
	}

	Ok(())
}

async fn unixserver<S: AsRef<str>>(connection: Connection, listener: UnixListener, domain: S, hostmaster: S) -> Result<()>
{
	let soa_record = &ResponseEntry::soa(&domain, &hostmaster);

	listener.incoming().map(|res| res.map_err(|err| ErrorKind::UnixServerError(Box::new(err.into())))).try_for_each_concurrent(10, |stream|
	{
		debug!("connection on unix socket");
		let mut writer = stream;
		let channel = connection.create_channel();
		let reader = BufReader::new(writer.clone());

		let domain = domain.as_ref().to_string();

		trace!("starting async task");
		async move
		{
			let channel = channel.await.chain_err(|| ErrorKind::MessageQueueChannelTaint)?;
			trace!("async task running");
			let mut lines = reader.split(b'\n');
			while let Some(input) = lines.next().await
			{
				trace!("request on unix domain socket");
				let input = match input
				{
					Err(err) =>
					{
						warn!("unix connection erred: {}", err);
						break;
					},
					Ok(ok) => ok,
				};

				trace!("parsing request");
				match serde_json::from_slice::<Query>(&input)
				{
					Ok(Query::Lookup { parameters: query, }) => 
					{
						debug!("query: {} ({})", query.qname(), query.qtype());

						match query.type_for_domain(&domain)
						{
							LookupType::SendAcme { soa, domain, } =>
							{
								debug!("acme-challenge response for {}: {} {} SOA", query.qname(), domain, if soa { "with" } else { "without" });

								let mut vec = vec![ResponseEntry::ns(query.qname(), domain)];

								if soa
								{
									vec.push(soa_record.clone());
								}

								match serde_json::to_string(&Response::from(vec))
								{
									Ok(json) =>
									{
										if let Err(err) = writeln!(writer, "{}", json).await
										{
											info!("closing unix stream due to error: {}", err);
											break;
										}
									},
									Err(err) => 
									{
										info!("closing unix stream due to error: {}", err);
										break;
									},
								}
							},
							LookupType::SendAaaa { soa, container, domain, } =>
							{
								debug!("querying for {}", container.name());
								match remote_query(&channel,&container).await
								{
									Ok(result) =>
									{
										debug!("query for {} yielded: {:?}", container.name(), result);
										let mut vec = Vec::new();
										if soa
										{
											vec.push(soa_record.clone());
										}

										if let Some(addresses) = result
										{
											vec.extend(addresses.into_iter()
												.map(|address| ResponseEntry::aaaa(&domain, address))
											);
										}

										match serde_json::to_string(&Response::from(vec))
										{
											Ok(json) =>
											{
												if let Err(err) = writeln!(writer, "{}", json).await
												{
													info!("closing unix stream due to error: {}", err);
													break;
												}
											},
											Err(err) => 
											{
												info!("closing unix stream due to error: {}", err);
												break;
											},
										}
									},
									Err(err) =>
									{
										error!("channel yielded error resolving {}, assuming taint: {}", domain, err);
										Err(err).chain_err(|| ErrorKind::MessageQueueChannelTaint)?;
									},
								}
							},
							LookupType::SendSoa(domain) =>
							{
								debug!("sending soa for {}", domain);
								match serde_json::to_string(&Response::from(vec![soa_record.clone()]))
								{
									Ok(json) =>
									{
										if let Err(err) = writeln!(writer, "{}", json).await
										{
											info!("closing unix stream due to error: {}", err);
											break;
										}
									},
									Err(err) => 
									{
										info!("closing unix stream due to error: {}", err);
										break;
									},
								}
							},
							LookupType::WrongDomain(domain) =>
							{
								debug!("request for unknown domain: {}", domain);
								if let Err(err) = writeln!(writer, r#"{{"result": false}}"#).await
								{
									info!("closing unix stream due to error: {}", err);
									break;
								}
							},
							LookupType::Unknown { domain, qtype, } =>
							{
								debug!("unknown request: {} ({})", domain, qtype);
								if let Err(err) = writeln!(writer, "{}", json!({ "result": true }).to_string()).await
								{
									info!("closing unix stream due to error: {}", err);
									break;
								}
							},
						}
					},
					Ok(Query::Initialize) =>
					{
						if let Err(err) = writeln!(writer, "{}", json!({ "result": true }).to_string()).await
						{
							info!("closing unix stream due to error: {}", err);
							break;
						}
					},
					Ok(Query::Unknown) =>
					{
						debug!("unknown method: {:?}", String::from_utf8_lossy(&input));
						if let Err(err) = writeln!(writer, "{}", json!({ "result": false }).to_string()).await
						{
							info!("closing unix connection due to error: {}", err);
							break;
						}
					},
					Err(err) =>
					{
						info!("error parsing request: {}", err);
						break;
					},
				}
			}
			debug!("unix connection closed");
			Ok(())
		}
	}).await?;
	Ok(())
}

#[async_std::main]
async fn main() -> !
{
	let matches = app_from_crate!()
		.arg(Arg::with_name("url")
			.short("u")
			.long("url")
			.help("connection string for the message queue")
			.takes_value(true)
			.env("LXDDNS_URL")
			.value_name("AMQP_URL")
			.default_value("amqp://guest:guest@[::1]:5672")
			.multiple(false)
		)
		.arg(Arg::with_name("loglevel")
			.short("v")
			.long("loglevel")
			.help("loglevel to be used, if not specified uses env_logger's auto-detection")
			.takes_value(true)
			.value_name("LOGLEVEL")
			.multiple(false)
		)
		.arg(Arg::with_name("hostmaster")
			.short("h")
			.long("hostmaster")
			.help("hostmaster to announce in SOA (use dot notation including trailing dot as in hostmaster.example.org.)")
			.takes_value(true)
			.value_name("SOA_HOSTMASTER")
			.multiple(false)
			.required(true)
		)
		.arg(Arg::with_name("domain")
			.short("d")
			.long("domain")
			.help("domain under which to run (do not forget the trailing dot)")
			.takes_value(true)
			.value_name("DOMAIN")
			.multiple(false)
			.required(true)
		)
		.arg(Arg::with_name("socket")
			.short("s")
			.long("socket")
			.help("location of the unix domain socket to be created")
			.takes_value(true)
			.value_name("SOCKET_PATH")
			.default_value("/var/run/lxddns/lxddns.sock")
			.multiple(false)
		)
		.get_matches();

	if let Some(loglevel) = matches.value_of("loglevel")
	{
		std::env::set_var("RUST_LOG", loglevel);
	}

	env_logger::init();
	info!("logging initialised");

	let url = matches.value_of("url").unwrap();
	let domain = matches.value_of("domain").unwrap();
	let hostmaster = matches.value_of("hostmaster").unwrap();
	let unixpath = Path::new(matches.value_of("socket").unwrap());

	loop
	{
		info!("running all services");
		match run(&unixpath, &url, &domain, &hostmaster).await
		{
			Ok(_) => unreachable!(),
			Err(err) =>
			{
				error!("fatal error occured: {}",err);
				for err in err.iter()
				{
					error!("caused by: {}",err);
				}
				error!("restarting all services");
			},
		}
		let _ = remove_file(&unixpath).await;
		task::sleep(Duration::from_secs(1)).await;
	}
}

async fn run<S: AsRef<str>,P: AsRef<Path>>(unixpath: P, url: S, domain: S, hostmaster: S) -> Result<()> // use never when available
{
	let domain = domain.as_ref().to_string();
	let hostmaster = hostmaster.as_ref().to_string();

	let connection = Connection::connect(url.as_ref(),Default::default()).await?;
	info!("connection to message queue established");

	let channel = connection.create_channel().await?;
	debug!("channel created");
	let responder = task::spawn_local(async move { responder(channel).await });
	info!("responder spawned");

	let listener = UnixListener::bind(unixpath.as_ref()).await?;
	info!("unix socket opened");

	let unixserver = task::spawn_local(async move { unixserver(connection,listener,domain,hostmaster).await });
	info!("unixserver started");

	info!("running");
	match future::select(unixserver,responder).await
	{
		future::Either::Left((Ok(()), _)) => Err(ErrorKind::UnixServerClosed.into()),
		future::Either::Left((Err(err), _)) => Err(ErrorKind::UnixServerError(Box::new(err)).into()),
		future::Either::Right((Ok(()), _)) => Err(ErrorKind::ResponderClosed.into()),
		future::Either::Right((Err(err), _)) => Err(ErrorKind::ResponderError(Box::new(err)).into()),
	}
}

