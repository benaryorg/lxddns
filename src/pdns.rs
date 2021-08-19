use super::
{
	Deserialize,
	Deserializer,
	Getters,
	Serialize,
	ContainerName,
	Ipv6Addr,
	trace,
	debug,
};

pub enum LookupType
{
	Smart
	{
		container: ContainerName,
		response: SmartResponse
	},
	Dumb
	{
		response: DumbResponse,
	},
}

pub enum SmartResponse
{
	Aaaa
	{
		soa: bool,
	},
	Soa,
}

impl SmartResponse
{
	pub fn response<S: AsRef<str>>(self, qname: S, soa: &ResponseEntry, addresses: Option<Vec<Ipv6Addr>>) -> Response
	{
		trace!("[smartresponse][{}] response", qname.as_ref());

		match self
		{
			SmartResponse::Soa =>
			{
				trace!("[smartresponse][{}][soa] checking addresses", qname.as_ref());

				if addresses.is_some()
				{
					debug!("[smartresponse][{}][soa] adding soa", qname.as_ref());

					vec![soa.clone()].into()
				}
				else
				{
					debug!("[smartresponse][{}][soa] sending nxdomain", qname.as_ref());

					DumbResponse::Nxdomain.response(qname, soa)
				}
			},
			SmartResponse::Aaaa { soa: send_soa, } =>
			{
				trace!("[smartresponse][{}][aaaa] checking addresses", qname.as_ref());

				if let Some(addresses) = addresses
				{
					trace!("[smartresponse][{}][aaaa] has addresses, building response", qname.as_ref());

					let mut vec = addresses.into_iter()
						.map(|addr| ResponseEntry::aaaa(qname.as_ref().clone(), addr))
						.collect::<Vec<_>>();

					if send_soa
					{
						trace!("[smartresponse][{}][aaaa] adding soa", qname.as_ref());
						vec.push(soa.clone());
					}

					vec.into()
				}
				else
				{
					trace!("[smartresponse][{}][aaaa] sending nxdomain", qname.as_ref());
					DumbResponse::Nxdomain.response(qname, soa)
				}
			},
		}
	}
}

pub enum DumbResponse
{
	Acme
	{
		target: String,
	},
	Nxdomain,
	Soa,
}

impl DumbResponse
{
	pub fn response<S: AsRef<str>>(self, qname: S, soa: &ResponseEntry) -> Response
	{
		trace!("[dumbresponse][{}] responding", qname.as_ref());
		match self
		{
			DumbResponse::Acme { target, } =>
			{
				debug!("[dumbresponse][{}][acme] responding with {}", qname.as_ref(), target);
				vec![ResponseEntry::ns(qname, target)].into()
			},
			DumbResponse::Nxdomain =>
			{
				debug!("[dumbresponse][{}][nxdomain] responding", qname.as_ref());
				vec![].into()
			},
			DumbResponse::Soa =>
			{
				debug!("[dumbresponse][{}][soa] responding", qname.as_ref());
				vec![soa.clone()].into()
			},
		}
	}
}

/// PowerDNS request structure.
///
/// A request for records by PowerDNS.
/// This struct contains the logic of when to send which record, although it works in tandem with [`DumbResponse`] and [`SmartResponse`].
///
/// Moving this into its own structure, while providing some level of maintainability mainly allows for proper unit testing.
/// The unit testing done for this struct however also tests [`DumbResponse`] and [`SmartResponse`] at the same time.
///
/// # Tests
///
/// ```
/// assert!(true);
/// ```
#[derive(Getters,Deserialize,Clone,Eq,PartialEq,Hash,Debug)]
pub struct QueryParameters
{
	#[serde(deserialize_with = "deserialize_string_lowercase")]
	#[get = "pub"]
	qname: String,
	#[get = "pub"]
	qtype: String,
	#[serde(default)]
	#[get = "pub"]
	zone_id: isize,
	// unused: remote, local, real-remote
}

impl QueryParameters
{
	pub fn type_for_domain<S: AsRef<str>>(&self, domain: S) -> LookupType
	{
		trace!("[queryparameters][type_for_domain][{}][{}] parsing for {}", self.qname(), self.qtype(), domain.as_ref());

		let suffix = format!(".{}", domain.as_ref());
		if let Some(record) = self.qname.strip_suffix(&suffix)
		{
			trace!("[queryparameters][type_for_domain][{}][{}] is correct domain", self.qname(), self.qtype());

			if let Some(record) = record.strip_prefix("_acme-challenge.")
			{
				trace!("[queryparameters][type_for_domain][{}][{}] is acme-challenge", self.qname(), self.qtype());

				if let Ok(container) = record.parse::<ContainerName>()
				{
					trace!("[queryparameters][type_for_domain][{}][{}] is acme for valid container", self.qname(), self.qtype());

					if self.qtype().eq("SOA")
					{
						debug!("[queryparameters][type_for_domain][{}][{}] is soa on acme domain for valid container", self.qname(), self.qtype());

						LookupType::Dumb
						{
							response: DumbResponse::Nxdomain,
						}
					}
					else
					{
						debug!("[queryparameters][type_for_domain][{}][{}] is non-soa on acme domain for valid container", self.qname(), self.qtype());

						LookupType::Dumb
						{
							response: DumbResponse::Acme
							{
								target: format!("{}.{}", container.as_ref(), domain.as_ref())
							},
						}
					}
				}
				else
				{
					debug!("[queryparameters][type_for_domain][{}][{}] is acme for invalid container", self.qname(), self.qtype());

					LookupType::Dumb
					{
						response: DumbResponse::Nxdomain,
					}
				}
			}
			else
			{
				trace!("[queryparameters][type_for_domain][{}][{}] is not acme", self.qname(), self.qtype());

				if let Ok(container) = record.parse::<ContainerName>()
				{
					trace!("[queryparameters][type_for_domain][{}][{}] is valid container", self.qname(), self.qtype());

					if self.qtype().eq("ANY") || self.qtype().eq("AAAA")
					{
						debug!("[queryparameters][type_for_domain][{}][{}] is aaaa-ish container", self.qname(), self.qtype());

						LookupType::Smart
						{
							container: container,
							response: SmartResponse::Aaaa
							{
								soa: self.qtype().eq("ANY"),
							},
						}
					}
					else
					{
						debug!("[queryparameters][type_for_domain][{}][{}] is not aaaa-ish", self.qname(), self.qtype());

						LookupType::Dumb
						{
							response: DumbResponse::Soa,
						}
					}
				}
				else
				{
					debug!("[queryparameters][type_for_domain][{}][{}] is not a valid container", self.qname(), self.qtype());

					LookupType::Dumb
					{
						response: DumbResponse::Nxdomain,
					}
				}
			}
		}
		else
		{
			if self.qname.eq(domain.as_ref())
			{
				debug!("[queryparameters][type_for_domain][{}][{}] is exactly our domain", self.qname(), self.qtype());

				LookupType::Dumb
				{
					response: DumbResponse::Soa,
				}
			}
			else
			{
				debug!("[queryparameters][type_for_domain][{}][{}] is not our domain", self.qname(), self.qtype());

				LookupType::Dumb
				{
					response: DumbResponse::Nxdomain,
				}
			}
		}
	}
}

#[derive(Deserialize,Clone,Eq,PartialEq,Hash,Debug)]
#[serde(tag = "method")]
pub enum Query
{
	#[serde(rename = "initialize")]
	Initialize,
	#[serde(rename = "lookup")]
	Lookup
	{
		parameters: QueryParameters,
	},
	#[serde(other)]
	Unknown,
}

#[derive(Getters,Serialize,Clone,Eq,PartialEq,Hash,Debug)]
pub struct ResponseEntry
{
	#[get = "pub"]
	qtype: String,
	#[get = "pub"]
	qname: String,
	#[get = "pub"]
	content: String,
	#[get = "pub"]
	ttl: usize,
	// unused: domain_id,scopeMask,auth
}

impl ResponseEntry
{
	pub fn soa<D: AsRef<str>, H: AsRef<str>>(domain: D, hostmaster: H) -> Self
	{
		ResponseEntry
		{
			content: format!("{} {} 1 86400 7200 3600000 3600", domain.as_ref(), hostmaster.as_ref()),
			qtype: "SOA".to_string(),
			qname: domain.as_ref().to_string(),
			ttl: 512,
		}
	}

	pub fn ns<D: AsRef<str>, H: AsRef<str>>(domain: D, target: H) -> Self
	{
		ResponseEntry
		{
			content: target.as_ref().to_string(),
			qtype: "NS".to_string(),
			qname: domain.as_ref().to_string(),
			ttl: 7200,
		}
	}

	pub fn aaaa<D: AsRef<str>>(domain: D, addr: Ipv6Addr) -> Self
	{
		ResponseEntry
		{
			content: format!("{}", addr),
			qtype: "AAAA".to_string(),
			qname: domain.as_ref().to_string(),
			ttl: 16,
		}
	}
}

#[derive(Getters,Serialize,Clone,Eq,PartialEq,Hash,Debug)]
pub struct Response
{
	#[get = "pub"]
	result: Vec<ResponseEntry>,
}

impl From<Vec<ResponseEntry>> for Response
{
	fn from(entries: Vec<ResponseEntry>) -> Self
	{
		Response
		{
			result: entries,
		}
	}
}

fn deserialize_string_lowercase<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
	where
		D: Deserializer<'de>
{
	let mut string = String::deserialize(deserializer)?;
	string.make_ascii_lowercase();
	Ok(string)
}

