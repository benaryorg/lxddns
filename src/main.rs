mod error;
use error::*;

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
		Serialize,
	},
	serde_json::
	{
		Value,
		json,
	},
	uuid::Uuid,
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

#[derive(Deserialize,Clone,Eq,PartialEq,Hash,Debug)]
struct CpuState
{
	usage: u128,
}

#[derive(Deserialize,Clone,Eq,PartialEq,Hash,Debug)]
struct DiskState
{
	usage: u128,
}

#[derive(Deserialize,Clone,Eq,PartialEq,Hash,Debug)]
struct MemoryState
{
	swap_usage: u128,
	swap_usage_peak: u128,
	usage: u128,
	usage_peak: u128,
}

#[derive(Deserialize,Clone,Eq,PartialEq,Hash,Debug)]
enum AddressFamily
{
	#[serde(rename = "inet6")]
	Inet6,
	#[serde(rename = "inet")]
	Inet,
}

#[derive(Deserialize,Clone,Eq,PartialEq,Hash,Debug)]
enum AddressScope
{
	#[serde(rename = "local")]
	Local,
	#[serde(rename = "global")]
	Global,
	#[serde(rename = "link")]
	Link,
}

#[derive(Deserialize,Clone,Eq,PartialEq,Hash,Debug)]
struct Address
{
	address: String,
	family: AddressFamily,
	scope: AddressScope,
	netmask: String,
}

#[derive(Deserialize,Clone,Eq,PartialEq,Hash,Debug)]
struct NetCounters
{
	bytes_received: u128,
	bytes_sent: u128,
	packets_received: u128,
	packets_sent: u128,
}

#[derive(Deserialize,Clone,Eq,PartialEq,Hash,Debug)]
struct NetState
{
	addresses: Vec<Address>,
	counters: NetCounters,
	host_name: String,
	hwaddr: String,
	mtu: usize,
	state: String,
	// too lazy to find a workaround
	// type: String,
}

#[derive(Deserialize,Clone,Eq,PartialEq,Debug)]
struct ContainerState
{
	pid: usize,
	processes: usize,
	// probably breaks if enum
	status: String,
	status_code: usize,
	cpu: CpuState,
	disk: HashMap<String,DiskState>,
	network: HashMap<String,NetState>,
	memory: MemoryState,
}

#[derive(Hash,Clone,Eq,Ord,PartialEq,PartialOrd,Debug)]
struct ContainerName(String);

impl AsRef<str> for ContainerName
{
	fn as_ref(&self) -> &str
	{
		self.0.as_str()
	}
}

impl FromStr for ContainerName
{
	type Err = Error;

	fn from_str(name: &str) -> Result<Self>
	{
		if !static_regex!(r"\A[-a-z0-9]+\z").is_match(&name)
		{
			bail!(ErrorKind::UnsafeName(name.to_string()))
		}
		Ok(Self(name.to_string()))
	}
}

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

	if state.status != "Running"
	{
		trace!("local query got says {} is not running",name.as_ref());
		return Ok(None);
	}

	let addresses = state.network
		.values()
		.flat_map(|net| net.addresses.iter())
		.filter(|address| address.scope == AddressScope::Global && address.family == AddressFamily::Inet6)
		.filter_map(|address| address.address.parse::<Ipv6Addr>().ok())
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

#[derive(Deserialize,Clone,Eq,PartialEq,Hash,Debug)]
struct QueryParameters
{
	qtype: String,
	qname: String,
	// *not* optional -.- // zone_id: isize,
	// unused: remote, local, real-remote
}

#[derive(Deserialize,Clone,Eq,PartialEq,Hash,Debug)]
struct Query
{
	method: String,
	parameters: QueryParameters,
}

#[derive(Serialize,Clone,Eq,PartialEq,Hash,Debug)]
struct ResponseEntry
{
	qtype: String,
	qname: String,
	content: String,
	ttl: usize,
	// unused: domain_id,scopeMask,auth
}

#[derive(Serialize,Clone,Eq,PartialEq,Hash,Debug)]
struct Response
{
	result: Vec<ResponseEntry>,
}

async fn unixserver(connection: Connection, listener: UnixListener) -> Result<()>
{
	let ref soa = ResponseEntry
	{
		content: "lxd.bsocat.net. hostmaster.benary.org. 1 86400 7200 3600000 3600".to_string(),
		qtype: "SOA".to_string(),
		qname: "lxd.bsocat.net.".to_string(),
		ttl: 512,
	};

	listener.incoming().map(|res| res.chain_err(|| ErrorKind::MessageQueueChannelTaint)).try_for_each_concurrent(10, |stream|
	{
		debug!("connection on unix socket");
		let mut writer = stream;
		let channel = connection.create_channel();
		let reader = BufReader::new(writer.clone());

		trace!("starting async task");
		async move
		{
			let mut channel = channel.await.chain_err(|| ErrorKind::MessageQueueChannelTaint)?;
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
						continue;
					},
					Ok(ok) => ok,
				};

				trace!("parsing request");
				match serde_json::from_slice::<Value>(&input)
				{
					Ok(Value::Object(obj)) =>
					{
						match obj.get("method")
						{
							Some(Value::String(method)) =>
							{
								trace!("request has method {}", method);
								match method.as_str()
								{
									"lookup" => 
									{
										match serde_json::from_slice::<Query>(&input)
										{
											Ok(Query { parameters: QueryParameters { qname, qtype, .. }, .. }) if qname.split('.').count() == 6 && qname.starts_with("_acme-challenge.") && qname.ends_with(".lxd.bsocat.net.") =>
											{
												debug!("acme-challenge request for {} ({})", qname, qtype);
												let parts = qname.split('.').collect::<Vec<_>>();
												let iscontainer = parts.get(1).unwrap().parse::<ContainerName>().is_ok();
												let containerdomain = parts.into_iter().skip(1).collect::<Vec<_>>().join(".");
												debug!("responding to acme-challenge query for {} with NS {}", qname, containerdomain);

												let mut vec = Vec::new();

												if iscontainer && qtype != "SOA"
												{
													vec.push(ResponseEntry
													{
														content: containerdomain.clone(),
														qtype: "NS".to_string(),
														qname: qname.to_string(),
														ttl: 7200,
													});
													debug!("acme-challenge ({}) for container name {}", qtype, qname);
												}

												if !vec.is_empty()
												{
													match serde_json::to_value(Response { result: vec, })
													{
														Ok(response) =>
														{
															if let Err(err) = writeln!(writer, "{}", response.to_string()).await
															{
																info!("closing unix connection due to error: {}", err);
																break;
															}
															else
															{
																debug!("sent reply");
																continue;
															}
														}
														Err(err) => info!("cannot create JSON value: {}", err),
													}
												}
												else
												{
													debug!("no results for {} ({}), not sending response", qname, qtype)
												}
											},
											Ok(Query { parameters: QueryParameters { qname, qtype, .. }, .. }) if (qtype == "AAAA" || qtype == "ANY") && qname.ends_with(".lxd.bsocat.net.") && qname.split('.').count() == 5 =>
											{
												trace!("request for {}", qname);
												match qname.split('.').next().unwrap().parse::<ContainerName>()
												{
													Ok(name) =>
													{
														debug!("querying for {}", name.as_ref());
														match remote_query(&mut channel,&name).await
														{
															Ok(result) =>
															{
																debug!("query for {} yielded: {:?}", name.as_ref(), result);
																let mut vec = Vec::new();
																if qtype == "ANY"
																{
																	vec.push(soa.clone());
																}

																if let Some(addresses) = result
																{
																	vec.extend(addresses.into_iter()
																		.map(|address| ResponseEntry
																		{
																			content: format!("{}", address),
																			qtype: "AAAA".to_string(),
																			qname: qname.to_string(),
																			ttl: 32,
																		})
																	);
																}

																if !vec.is_empty()
																{
																	match serde_json::to_value(Response { result: vec, })
																	{
																		Ok(response) =>
																		{
																			if let Err(err) = writeln!(writer, "{}", response.to_string()).await
																			{
																				info!("closing unix connection due to error: {}", err);
																				break;
																			}
																			else
																			{
																				debug!("sent reply");
																				continue;
																			}
																		}
																		Err(err) => info!("cannot create JSON value: {}", err),
																	}
																}
																else
																{
																	debug!("no results for {} ({}), not sending response", qname, qtype)
																}
															},
															Err(err) =>
															{
																error!("channel yielded error resolving {}, assuming taint: {}", name.as_ref(), err);
																Err(err).chain_err(|| ErrorKind::MessageQueueChannelTaint)?;
															},
														}
													},
													Err(err) => info!("not a containername: {}",err),
												}
											},
											Ok(Query { parameters: QueryParameters { qtype, qname, .. }, .. }) if (qtype == "SOA" || qtype == "ANY") && (qname.ends_with(".lxd.bsocat.net.") || qname == "lxd.bsocat.net.") =>
											{
												match serde_json::to_value(Response { result: vec![soa.clone()], })
												{
													Ok(response) =>
													{
														if let Err(err) = writeln!(writer, "{}", response.to_string()).await
														{
															info!("closing unix connection due to error: {}", err);
															break;
														}
														else
														{
															debug!("sent the SOA record for {}", qname);
															continue;
														}
													}
													Err(err) => info!("cannot create JSON value: {}", err),
												}
											},
											Ok(Query { parameters: QueryParameters { qtype, qname, .. }, .. }) => debug!("no response for: {} ({})", qtype, qname),
											Err(err) => info!("could not parse query: {}",err),
										}
									},
									"initialize" =>
									{
										if let Err(err) = writeln!(writer,"{}",json!({ "result": true }).to_string()).await
										{
											info!("closing unix connection due to error: {}", err);
											break;
										}
										else
										{
											continue;
										}
									}
									_ => info!("unknown method: {}", method),
								}
							},
							Some(_) => info!("method not string"),
							None => info!("no method provided"),
						}
					}
					Ok(_) => info!("input not an object"),
					Err(err) => info!("input not JSON: {}", err),
				}

				if let Err(err) = writeln!(writer, "{}", json!({ "result": false }).to_string()).await
				{
					info!("closing unix connection due to error: {}", err);
					break;
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
			.value_name("AMQP_URL")
			.default_value("amqp://guest:guest@[::1]:5672")
			.multiple(false)
			.global(true)
		)
		.arg(Arg::with_name("loglevel")
			.short("v")
			.long("loglevel")
			.help("loglevel to be used, if not specified uses env_logger's auto-detection")
			.takes_value(true)
			.value_name("LOGLEVEL")
			.multiple(false)
			.global(true)
		)
		.arg(Arg::with_name("socket")
			.short("s")
			.long("socket")
			.help("location of the unix domain socket to be created")
			.takes_value(true)
			.value_name("SOCKET_PATH")
			.default_value("/var/run/lxddns/lxddns.sock")
			.multiple(false)
			.global(true)
		)
		.get_matches();

	if let Some(loglevel) = matches.value_of("loglevel")
	{
		std::env::set_var("RUST_LOG", loglevel);
	}

	env_logger::init();
	info!("logging initialised");

	let url = matches.value_of("url").unwrap();
	let unixpath = Path::new(matches.value_of("socket").unwrap());

	loop
	{
		info!("running all services");
		match run(&unixpath, &url).await
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

async fn run<S: AsRef<str>,P: AsRef<Path>>(unixpath: P, url: S) -> Result<()> // use never when available
{
	let connection = Connection::connect(url.as_ref(),Default::default()).await?;
	info!("connection to message queue established");

	let channel = connection.create_channel().await?;
	debug!("channel created");
	let responder = task::spawn_local(async move { responder(channel).await });
	info!("responder spawned");

	let listener = UnixListener::bind(unixpath.as_ref()).await?;
	info!("unix socket opened");

	let unixserver = task::spawn_local(async move { unixserver(connection,listener).await });
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

