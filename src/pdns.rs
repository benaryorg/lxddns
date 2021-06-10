use super::
{
	Deserialize,
	Deserializer,
	Getters,
	Serialize,
	ContainerName,
	Ipv6Addr,
};

pub enum LookupType
{
	SendAaaa
	{
		soa: bool,
		domain: String,
		container: ContainerName,
	},
	SendAcme
	{
		soa: bool,
		domain: String,
	},
	SendSoa(String),
	WrongDomain(String),
	Unknown
	{
		domain: String,
		qtype: String,
	},
}

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
		let domain_dots = domain.as_ref().chars().filter(|&ch| ch == '.').count();
		let num_dots = self.qname.chars().filter(|&ch| ch == '.').count();
		let suffix = format!(".{}", domain.as_ref());

		// make sure this instance is supposed to answer
		//  the rest of the code assumes SOA for the zone is acceptable
		if !self.qname.ends_with(&suffix) && self.qname != domain.as_ref()
		{
			return LookupType::WrongDomain(self.qname.clone());
		}

		if self.qname.starts_with("_acme-challenge.")
		{
			// hierarchy does not match
			if num_dots != domain_dots + 2
			{
				return LookupType::SendSoa(self.qname.to_string())
			}

			let parts = self.qname.split('.').collect::<Vec<_>>();
			let iscontainer = parts.get(1).unwrap().parse::<ContainerName>().is_ok();
			let containerdomain = parts.into_iter().skip(1).collect::<Vec<_>>().join(".");

			// not asking for a container
			if !iscontainer || self.qtype == "SOA"
			{
				return LookupType::Unknown
				{
					domain: self.qname.clone(),
					qtype: self.qtype.clone(),
				};
			}

			return LookupType::SendAcme
			{
				soa: false,
				domain: containerdomain,
			}
		}

		// handle valid-ish AAAA
		if (self.qtype == "AAAA" || self.qtype == "ANY") && num_dots == domain_dots + 1
		{
			// does it look like a container?
			if let Ok(name) = self.qname.split('.').next().unwrap().parse::<ContainerName>()
			{
				// needs to be resolved
				return LookupType::SendAaaa
				{
					soa: self.qtype == "ANY",
					domain: self.qname.clone(),
					container: name,
				}
			}
			else
			{
				return LookupType::SendSoa(self.qname.clone())
			}
		}

		// anything that requires SOA
		if self.qtype == "SOA" || self.qtype == "ANY"
		{
			return LookupType::SendSoa(self.qname.clone());
		}

		// everything else is strange
		return LookupType::Unknown
		{
			domain: self.qname.clone(),
			qtype: self.qtype.clone(),
		};
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

