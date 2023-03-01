use crate::
{
	error::*,
	lxd::
	{
		ContainerName,
	},
};

use ::
{
	lapin::
	{
		Connection,
		ExchangeKind,
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
		warn,
		info,
		debug,
		trace,
	},
	futures::
	{
		stream::
		{
			TryStreamExt,
		},
	},
	std::
	{
		sync::Arc,
	},
};

pub struct Responder
{
	connection: Connection,
	queue_name: String,
	responder_workers: usize,
}

impl Responder
{
	pub fn builder() -> ResponderBuilder
	{
		Default::default()
	}

	pub async fn run(self) -> Result<()>
	{
		let channel = self.connection.create_channel().await.context(Error::QueueConnectionError)?;

		channel.exchange_declare("lxddns", ExchangeKind::Fanout, Default::default(), Default::default()).await?;
		trace!("[responder] created fanout exchange");

		let queue = channel.queue_declare(
			&self.queue_name,
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
		trace!("[responder] bound exchange to queue {}", queue.name());

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

		let me = Arc::new(&self);

		consumer.err_into::<anyhow::Error>().try_for_each_concurrent(self.responder_workers, |delivery|
		{
			let me = me.clone();

			async move
			{
				debug!("[responder] received message");
				let name = String::from_utf8_lossy(&delivery.data);
				debug!("[responder][{}] received request", name);

				let channel = me.connection.create_channel().await.context(Error::QueueConnectionError)?;
				debug!("[responder][{}] channel created", name);

				let name = match name.parse::<ContainerName>()
				{
					Ok(ok) => ok,
					Err(_) =>
					{
						info!("[responder][{}] invalid name; rejecting", name);
						delivery.acker.reject(BasicRejectOptions
						{
							requeue: false,
						}).await.context(Error::AcknowledgementError)?;
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
						}).await.context(Error::AcknowledgementError)?;
						return Ok(());
					}
				};

				let addresses = match crate::lxd::local_query(&name).await
				{
					Ok(Some(addresses)) =>
					{
						trace!("[responder][{}][{}] got {:?}", name.as_ref(), corr_id, addresses);

						addresses
					},
					Ok(_) =>
					{
						trace!("[responder][{}] no info; rejecting", name.as_ref());
						delivery.acker.reject(BasicRejectOptions
						{
							requeue: false,
						}).await.context(Error::AcknowledgementError)?;
						return Ok(());
					},
					Err(err) =>
					{
						warn!("[responder][{}][{}] query error: {}", name.as_ref(), corr_id, err);
						for err in err.chain().skip(1)
						{
							warn!("[responder][{}][{}]  caused by: {}", name.as_ref(), corr_id, err);
						}
						delivery.acker.reject(BasicRejectOptions
						{
							requeue: true,
						}).await.context(Error::AcknowledgementError)?;
						return Ok(());
					},
				};
				let response = addresses.into_iter().flat_map(|addr| u128::from(addr).to_le_bytes().to_vec()).collect::<Vec<u8>>();

				channel.basic_publish("",reply_to.as_str(),Default::default(), &response,
					AMQPProperties::default()
						.with_correlation_id(corr_id.clone())
				).await.context("basic_publish")?;
				trace!("[responder][{}][{}] message published to {}", name.as_ref(), corr_id, reply_to);

				delivery.acker.ack(BasicAckOptions
				{
					multiple: false,
				}).await.context(Error::AcknowledgementError)?;

				Ok(())
			}
		}).await.context("responder loop error")?;

		Ok(())
	}
}

#[derive(Clone,Eq,PartialEq,Hash,Debug,Default)]
pub struct ResponderBuilder
{
	url: Option<String>,
	queue_name: Option<String>,
	responder_workers: Option<usize>,

}

impl ResponderBuilder
{
	pub fn url<S: AsRef<str>>(mut self, url: S) -> Self
	{
		self.url = Some(url.as_ref().into());
		self
	}

	pub fn queue_name<S: AsRef<str>>(mut self, queue_name: S) -> Self
	{
		self.queue_name = Some(queue_name.as_ref().into());
		self
	}

	pub fn responder_workers(mut self, responder_workers: usize) -> Self
	{
		self.responder_workers = Some(responder_workers);
		self
	}

	pub async fn run(self) -> Result<()>
	{
		let url = self.url.map(Result::Ok).unwrap_or_else(|| bail!("no url provided")).context(Error::InvalidConfiguration)?;
		let queue_name = self.queue_name.unwrap_or_default();
		let responder_workers = self.responder_workers.unwrap_or(8);

		let connection = Connection::connect(url.as_ref(), Default::default())
			.await
			.context("connect failed")
			.context(Error::QueueConnectionError)
		?;

		info!("[responder][run] connection to message queue established");

		Responder
		{
			connection,
			queue_name,
			responder_workers,
		}.run().await
	}
}

