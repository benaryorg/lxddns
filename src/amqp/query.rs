use crate::
{
	error::*,
	lxd::
	{
		ContainerName,
	},
	pdns_io::
	{
		RemoteQuery as RemoteQueryTrait,
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
	tokio::
	{
		time::timeout,
	},
	futures::
	{
		stream::
		{
			StreamExt,
		},
	},
	uuid::Uuid,
	std::
	{
		time::
		{
			Duration,
			Instant,
		},
		net::Ipv6Addr,
		convert::TryInto,
	},
};

pub struct RemoteQuery
{
	channel: Channel,
	response_queue: Queue,
}

impl RemoteQuery
{
	pub async fn new(channel: Channel) -> Result<Self>
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

		info!("[amqp-remote_query][new] connection to message queue {} established", response_queue.name());

		Ok(Self
		{
			channel,
			response_queue,
		})
	}
}

#[async_trait::async_trait]
impl RemoteQueryTrait for RemoteQuery
{
	fn name(&self) -> String
	{
		self.response_queue.name().to_string()
	}

	async fn remote_query(&self, name: &ContainerName) -> Result<Option<Vec<Ipv6Addr>>>
	{
		debug!("[amqp-remote_query][{}] starting query", name.as_ref());

		let mut consumer = self.channel.basic_consume(self.response_queue.name().as_str(),
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
		trace!("[amqp-remote_query][{}] correlation id: {}", name.as_ref(), correlation_id);

		self.channel.basic_publish("lxddns","lxddns",Default::default(),name.as_ref().as_bytes(),
			AMQPProperties::default()
				.with_correlation_id(format!("{}", correlation_id).into())
				.with_reply_to(self.response_queue.name().clone())
		).await.with_context(|| "error in basic_publish")?;
		trace!("[amqp-remote_query][{}][{}] published message", name.as_ref(), correlation_id);

		let mut result = None;

		// FIXME: this timeout needs to be configurable
		//  the timeout strongly depends on the latency between hosts, in my case ~250ms at most
		let mut timer = Duration::from_millis(2000);
		let extension = Duration::from_millis(250);
		let instant = Instant::now();

		while let Ok(Some(Ok(delivery))) = timeout(timer.saturating_sub(instant.elapsed()), consumer.next()).await
		{
			let elapsed = instant.elapsed();
			trace!("[amqp-remote_query][{}][{}] got response after {:.3}s", name.as_ref(), correlation_id, elapsed.as_secs_f64());

			let received_id = match delivery.properties.correlation_id()
			{
				Some(received_id) => received_id,
				None =>
				{
					info!("[amqp-remote_query][{}][{}] response without correlation_id; rejecting", name.as_ref(), correlation_id);
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
					info!("[amqp-remote_query][{}][{}] response with invalid correlation_id: {}; rejecting", name.as_ref(), correlation_id, received_id);
					delivery.acker.reject(BasicRejectOptions
					{
						requeue: false,
					}).await?;
					continue;
				},
			};

			if received_id.ne(&correlation_id)
			{
				debug!("[amqp-remote_query][{}][{}] unrelated message received; rejecting", name.as_ref(), correlation_id);
				delivery.acker.reject(BasicRejectOptions
				{
					requeue: false,
				}).await?;
				continue;
			}

			timer = elapsed + (elapsed + 2*extension)/2;

			if let Ok(addresses) = delivery.data.chunks(16)
				.map(|v| Ok(Ipv6Addr::from(u128::from_le_bytes(v.to_vec().try_into()?))))
				.collect::<std::result::Result<Vec<_>,Vec<_>>>()
			{
				debug!("[amqp-remote_query][{}][{}] got response after {:.3}s: {:?}", name.as_ref(), correlation_id, instant.elapsed().as_secs_f64(), addresses);
				result.get_or_insert_with(Vec::new).extend(addresses);
				delivery.acker.ack(BasicAckOptions
				{
					multiple: false,
				}).await?;
			}
			else
			{
				debug!("[amqp-remote_query][{}][{}] invalid content; rejecting", name.as_ref(), correlation_id);
				delivery.acker.reject(BasicRejectOptions
				{
					requeue: false,
				}).await?;
			}
		}

		Ok(result)
	}
}
