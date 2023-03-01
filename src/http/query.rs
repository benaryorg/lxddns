use crate::
{
	error::*,
	lxd::
	{
		ContainerName,
	},
	http::ApiResponse,
	pdns_io::
	{
		RemoteQuery as RemoteQueryTrait,
	},
};

use ::
{
	reqwest::
	{
		Client,
	},
	async_std::prelude::*,
	futures::
	{
		future::join_all,
	},
	std::
	{
		net::Ipv6Addr,
		time::
		{
			Duration,
			Instant,
		},
	},
};

pub struct RemoteQuery
{
	http: Client,
	remote: Vec<String>,
}

impl RemoteQuery
{
	pub async fn new(remote: Vec<String>) -> Result<Self>
	{
		info!("[http-remote_query][new] instantiated");

		Ok(Self
		{
			http: Client::builder()
				.timeout(Duration::from_millis(1500))
				.connect_timeout(Duration::from_millis(500))
				.build()?,
			remote,
		})
	}
}

#[async_trait::async_trait]
impl RemoteQueryTrait for RemoteQuery
{
	fn name(&self) -> String
	{
		"http-remote_query".to_string()
	}

	async fn remote_query(&self, name: &ContainerName) -> Result<Option<Vec<Ipv6Addr>>>
	{
		debug!("[http-remote_query][{}] starting query", name.as_ref());
		let instant = Instant::now();
		let requests = self.remote.iter()
			.map(|remote| self.http.get(format!("{}/resolve/v1/{}", remote, name.as_ref())).send())
			.collect::<Vec<_>>();

		let responses = join_all(requests).timeout(Duration::from_millis(4000)).await?;
		let mut result: Option<Vec<Ipv6Addr>> = None;
		for response in responses
		{
			// TODO: error handling
			trace!("[http-remote_query][{}]: {:?}", name.as_ref(), response);
			if let Ok(response) = response
			{
				if response.status().is_success()
				{
					if let Ok(response) = response.json::<ApiResponse>().await
					{
						if let ApiResponse::V1(Some(response)) = response
						{
							match result
							{
								Some(ref mut vec) => vec.extend(response),
								None => result = Some(response),
							}
						}
					}
				}
			}
		}

		debug!("[http-remote_query][{}] got response after {:.3}s: {:?}", name.as_ref(), instant.elapsed().as_secs_f64(), result);
		Ok(result)
	}
}
