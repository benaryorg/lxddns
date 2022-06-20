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
	},
};

use ::
{
	lapin::
	{
		Channel,
		Queue,
		options::
		{
			BasicAckOptions,
			BasicConsumeOptions,
			BasicRejectOptions,
			QueueDeclareOptions,
		},
		protocol::basic::AMQPProperties,
	},
	futures::
	{
		stream::
		{
			StreamExt,
		},
	},
	serde_json::
	{
		json,
	},
	uuid::Uuid,
	async_std::
	{
		prelude::*,
		io::
		{
			BufReader,
			Read,
			Write,
		},
	},
	std::
	{
		convert::TryInto,
		time::
		{
			Duration,
			Instant,
		},
		net::Ipv6Addr,
	},
};

pub struct PdnsStreamHandler<R, W>
	where
		R: Read+Unpin,
		W: Write+Unpin,
{
	domain: String,
	hostmaster: String,
	channel: Channel,
	response_queue: Queue,
	reader: R,
	writer: W,
}

impl<R, W> PdnsStreamHandler<R, W>
	where
		R: Read+Unpin,
		W: Write+Unpin,
{
	pub async fn new<S1, S2>(domain: S1, hostmaster: S2, channel: Channel, reader: R, writer: W) -> Result<Self>
		where
			S1: AsRef<str>,
			S2: AsRef<str>,
	{
		let response_queue = channel.queue_declare(
			"",
			QueueDeclareOptions
			{
				exclusive: false,
				auto_delete: false,
				..QueueDeclareOptions::default()
			},
			Default::default()
		).await.with_context(|| "error in queue_declare")?;

		info!("[pdns_io][handler] connection to message queue {} established", response_queue.name());

		Ok(Self
		{
			domain: domain.as_ref().to_string(),
			hostmaster: hostmaster.as_ref().to_string(),
			channel,
			reader,
			writer,
			response_queue,
		})
	}

	pub async fn run(mut self) -> Result<()>
	{
		let soa_record = &ResponseEntry::soa(&self.domain, &self.hostmaster);

		debug!("[pdns_io][handler] handling stream with queue {}", self.response_queue.name());

		let mut lines = BufReader::new(&mut self.reader).split(b'\n');
		while let Some(input) = lines.next().await
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
							let result = Self::remote_query(&container, &self.channel, &self.response_queue).timeout(Duration::from_millis(4500)).await;

							debug!("[pdns_io][handler][{}] remote_query ran for {:.3}s (timeout: {})", query.qname(), instant.elapsed().as_secs_f64(), result.is_err());

							match result.ok().unwrap_or(Ok(None))
							{
								Ok(result) =>
								{
									debug!("[pdns_io][handler][{}] got {:?}", query.qname(), result);

									let response = response.response(query.qname(), soa_record, result);

									match serde_json::to_string(&response)
									{
										Ok(json) =>
										{
											if let Err(err) = writeln!(self.writer, "{}", json).await
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

							let response = response.response(query.qname(), soa_record);

							match serde_json::to_string(&response)
							{
								Ok(json) =>
								{
									if let Err(err) = writeln!(self.writer, "{}", json).await
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
					if let Err(err) = writeln!(self.writer, "{}", json!({ "result": true })).await
					{
						warn!("[pdns_io][handler] closing unix stream due to socket error: {}", err);
						break;
					}
				},
				Ok(Query::Unknown) =>
				{
					debug!("[pdns_io][handler] unknown query: {:?}", String::from_utf8_lossy(&input));
					if let Err(err) = writeln!(self.writer, "{}", json!({ "result": false })).await
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
		}
		debug!("[pdns_io][handler] connection closed");

		Ok(())
	}

	async fn remote_query(name: &ContainerName, channel: &Channel, response_queue: &Queue) -> Result<Option<Vec<Ipv6Addr>>>
	{
		debug!("[remote_query][{}] starting query", name.as_ref());

		let mut consumer = channel.basic_consume(response_queue.name().as_str(),
			"",
			BasicConsumeOptions
			{
				no_ack: false,
				no_local: true,
				..Default::default()
			},
			Default::default()
		).await.with_context(|| "error in basic_consume")?;

		let correlation_id = Uuid::new_v4();
		trace!("[remote_query][{}] correlation id: {}", name.as_ref(), correlation_id);

		channel.basic_publish("lxddns","lxddns",Default::default(),name.as_ref().as_bytes(),
			AMQPProperties::default()
				.with_correlation_id(format!("{}", correlation_id).into())
				.with_reply_to(response_queue.name().clone())
		).await.with_context(|| "error in basic_publish")?;
		trace!("[remote_query][{}][{}] published message", name.as_ref(), correlation_id);

		let mut result = None;

		// FIXME: this timeout needs to be configurable
		//  the timeout strongly depends on the latency between hosts, in my case ~250ms at most
		let mut timeout = Duration::from_millis(2000);
		let extension = Duration::from_millis(250);
		let instant = Instant::now();

		while let Ok(Some(Ok(delivery))) = consumer.next().timeout(timeout.saturating_sub(instant.elapsed())).await
		{
			let elapsed = instant.elapsed();
			trace!("[remote_query][{}][{}] got response after {:.3}s", name.as_ref(), correlation_id, elapsed.as_secs_f64());

			let received_id = match delivery.properties.correlation_id()
			{
				Some(received_id) => received_id,
				None =>
				{
					info!("[remote_query][{}][{}] response without correlation_id; rejecting", name.as_ref(), correlation_id);
					delivery.acker.reject(BasicRejectOptions
					{
						requeue: false,
					}).await?;
					continue;
				},
			};

			let received_id = match Uuid::parse_str(received_id.as_str())
			{
				Ok(received_id) => received_id,
				Err(_) =>
				{
					info!("[remote_query][{}][{}] response with invalid correlation_id: {}; rejecting", name.as_ref(), correlation_id, received_id);
					delivery.acker.reject(BasicRejectOptions
					{
						requeue: false,
					}).await?;
					continue;
				},
			};

			if received_id.ne(&correlation_id)
			{
				debug!("[remote_query][{}][{}] unrelated message received; rejecting", name.as_ref(), correlation_id);
				delivery.acker.reject(BasicRejectOptions
				{
					requeue: false,
				}).await?;
				continue;
			}

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

		Ok(result)
	}
}

