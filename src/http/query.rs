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
	http::
	{
		ApiResponse,
		ApiResponseV1,
	},
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
	futures::
	{
		stream,
		FutureExt,
		StreamExt,
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
		debug!("[remote_query][{}] starting query", name.as_ref());
		let instant = Instant::now();
		let mut requests = stream::iter(self.remote.clone())
			.filter_map(|remote|
			{
				self.http.get(format!("{}/resolve/v1/{}", remote, name.as_ref())).send()
					.then(|response| Box::pin(async move
					{
						match response
						{
							Err(err) =>
							{
								warn!("[remote_query][{}] http error: {}", remote, err);
								debug!("[remote_query][{}] verbose http error: {:?}", remote, err);
								None
							},
							Ok(response) =>
							{
								let status = response.status();
								if !status.is_success()
								{
									warn!("[remote_query][{}] unexpected http response code: {}", remote, status);
									None
								}
								else
								{
									match response.json::<ApiResponse>().await
									{
										Ok(response) => Some(response),
										Err(err) =>
										{
											warn!("[remote_query][{}] json deserialization error: {}", remote, err);
											debug!("[remote_query][{}] verbose json deserialization error: {:?}", remote, err);
											None
										},
									}
								}
							}
						}
					}))
			})
		;

		let mut result: Option<Vec<Ipv6Addr>> = None;
		while let Some(response) = requests.next().await
		{
			trace!("[http-remote_query][{}]: {:?}", name.as_ref(), response);
			match response
			{
				ApiResponse::V1(ApiResponseV1::NoMatch) => {},
				ApiResponse::V1(ApiResponseV1::AnyMatch(response)) =>
				{
					match result
					{
						Some(ref mut vec) => vec.extend(response),
						None => result = Some(response),
					}
				},
			}
		}

		debug!("[http-remote_query][{}] got response after {:.3}s: {:?}", name.as_ref(), instant.elapsed().as_secs_f64(), result);
		Ok(result)
	}
}
