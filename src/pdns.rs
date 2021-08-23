#[allow(unused)]
use crate::error::*;

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

#[derive(Clone,Eq,PartialEq,Hash,Debug)]
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

#[derive(Clone,Eq,PartialEq,Hash,Debug)]
pub enum SmartResponse
{
	Aaaa,
}

impl SmartResponse
{
	pub fn response<S: AsRef<str>>(self, qname: S, soa: &ResponseEntry, addresses: Option<Vec<Ipv6Addr>>) -> Response
	{
		trace!("[smartresponse][{}] response", qname.as_ref());

		match self
		{
			SmartResponse::Aaaa =>
			{
				trace!("[smartresponse][{}][aaaa] checking addresses", qname.as_ref());

				if let Some(addresses) = addresses
				{
					trace!("[smartresponse][{}][aaaa] has addresses, building response", qname.as_ref());

					addresses.into_iter()
						.map(|addr| ResponseEntry::aaaa(qname.as_ref(), addr))
						.collect::<Vec<_>>()
						.into()
				}
				else
				{
					trace!("[smartresponse][{}][aaaa] sending nxdomain", qname.as_ref());
					DumbResponse::Soa.response(qname, soa)
				}
			},
		}
	}
}

#[derive(Clone,Eq,PartialEq,Hash,Debug)]
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
/// ## Base Domain
///
/// The base domain needs to resolve to a SOA entry.
///
/// ```
/// # use lxddns::pdns::*;
/// # use lxddns::lxd::*;
/// # use serde_json::{from_value, json};
/// let response = from_value::<QueryParameters>(json!(
/// {
///     "qname": "example.com",
///     "qtype": "SOA",
///     "zone_id": 0,
/// })).unwrap().type_for_domain("example.com");
///
/// assert_eq!(response, LookupType::Dumb
/// {
///     response: DumbResponse::Soa,
/// });
/// ```
///
/// ```
/// # use lxddns::pdns::*;
/// # use lxddns::lxd::*;
/// # use serde_json::{from_value, json};
/// let response = from_value::<QueryParameters>(json!(
/// {
///     "qname": "example.com",
///     "qtype": "ANY",
///     "zone_id": 0,
/// })).unwrap().type_for_domain("example.com");
///
/// assert_eq!(response, LookupType::Dumb
/// {
///     response: DumbResponse::Soa,
/// });
/// ```
///
/// ```
/// # use lxddns::pdns::*;
/// # use lxddns::lxd::*;
/// # use serde_json::{from_value, json};
/// let response = from_value::<QueryParameters>(json!(
/// {
///     "qname": "example.com",
///     "qtype": "AAAA",
///     "zone_id": 0,
/// })).unwrap().type_for_domain("example.com");
///
/// assert_eq!(response, LookupType::Dumb
/// {
///     response: DumbResponse::Soa,
/// });
/// ```
///
/// ## Container
///
/// Existing containers need to respond to AAAA and ANY with a AAAA when they exist, *NXDOMAIN* if they do not exist.
/// You'd expect them to also respond to SOA on SOA, but that would be a mistake as PowerDNS does not expect the backend to do anything of sorts.
/// PowerDNS magically figures out the SOA (and which domains exist and which don't) by querying the backend with multiple queries.
/// So from here on out, as the root zone is already established, everything that you'd expect to be SOA is actually *NXDOMAIN*.
/// For further information there is a [nice mailinglist thread on this matter](https://mailman.powerdns.com/pipermail/pdns-users/2019-February/025809.html).
///
/// ```
/// # use lxddns::pdns::*;
/// # use lxddns::lxd::*;
/// # use serde_json::{from_value, json};
/// let response = from_value::<QueryParameters>(json!(
/// {
///     "qname": "container.example.com",
///     "qtype": "SOA",
///     "zone_id": 0,
/// })).unwrap().type_for_domain("example.com");
///
/// assert_eq!(response, LookupType::Dumb
/// {
///     //response: DumbResponse::Soa, NOSOA
///     response: DumbResponse::Nxdomain,
/// });
/// ```
///
/// ```
/// # use lxddns::pdns::*;
/// # use lxddns::lxd::*;
/// # use serde_json::{from_value, json};
/// let response = from_value::<QueryParameters>(json!(
/// {
///     "qname": "container.example.com",
///     "qtype": "ANY",
///     "zone_id": 0,
/// })).unwrap().type_for_domain("example.com");
///
/// assert_eq!(response, LookupType::Smart
/// {
///     container: "container".parse().unwrap(),
///     response: SmartResponse::Aaaa,
/// });
/// ```
///
/// ```
/// # use lxddns::pdns::*;
/// # use lxddns::lxd::*;
/// # use serde_json::{from_value, json};
/// let response = from_value::<QueryParameters>(json!(
/// {
///     "qname": "container.example.com",
///     "qtype": "AAAA",
///     "zone_id": 0,
/// })).unwrap().type_for_domain("example.com");
///
/// assert_eq!(response, LookupType::Smart
/// {
///     container: "container".parse().unwrap(),
///     response: SmartResponse::Aaaa,
/// });
/// ```
///
/// ## ACME Domains
/// 
/// ACME Domains respond with NS entries on all requests except for SOA which are *NXDOMAIN*, otherwise PowerDNS fails.
///
/// ```
/// # use lxddns::pdns::*;
/// # use lxddns::lxd::*;
/// # use serde_json::{from_value, json};
/// let response = from_value::<QueryParameters>(json!(
/// {
///     "qname": "_acme-challenge.container.example.com",
///     "qtype": "SOA",
///     "zone_id": 0,
/// })).unwrap().type_for_domain("example.com");
///
/// assert_eq!(response, LookupType::Dumb
/// {
///     response: DumbResponse::Nxdomain,
/// });
/// ```
///
/// ```
/// # use lxddns::pdns::*;
/// # use lxddns::lxd::*;
/// # use serde_json::{from_value, json};
/// let response = from_value::<QueryParameters>(json!(
/// {
///     "qname": "_acme-challenge.container.example.com",
///     "qtype": "ANY",
///     "zone_id": 0,
/// })).unwrap().type_for_domain("example.com");
///
/// assert_eq!(response, LookupType::Dumb
/// {
///     response: DumbResponse::Acme
///     {
///         target: "container.example.com".to_string(),
///     },
/// });
/// ```
///
/// ```
/// # use lxddns::pdns::*;
/// # use lxddns::lxd::*;
/// # use serde_json::{from_value, json};
/// let response = from_value::<QueryParameters>(json!(
/// {
///     "qname": "_acme-challenge.container.example.com",
///     "qtype": "AAAA",
///     "zone_id": 0,
/// })).unwrap().type_for_domain("example.com");
///
/// assert_eq!(response, LookupType::Dumb
/// {
///     response: DumbResponse::Acme
///     {
///         target: "container.example.com".to_string(),
///     },
/// });
/// ```
///
/// ## Different Domain
///
/// Unrelated domains should be *REFUSED*, but for now we send *NXDOMAIN*.
///
/// ```
/// # use lxddns::pdns::*;
/// # use lxddns::lxd::*;
/// # use serde_json::{from_value, json};
/// for qname in
///     [ "example.org"
///     , "container.example.org"
///     , "_container.example.org"
///     , "_acme-challenge.container.example.org"
///     , "_acme-challenge._container.example.org"
///     ]
/// {
///     for qtype in
///         [ "SOA"
///         , "ANY"
///         , "AAAA"
///         ]
///     {
///         let response = from_value::<QueryParameters>(json!(
///         {
///             "qname": qname,
///             "qtype": qtype,
///             "zone_id": 0,
///         })).unwrap().type_for_domain("example.com");
///
///         assert_eq!(response, LookupType::Dumb
///         {
///             response: DumbResponse::Nxdomain,
///             //response: DumbResponse::Refused,
///         }, "wrong response for {} ({})", qname, qtype);
///     }
/// }
/// ```
///
/// ## Wrong Domains
///
/// Domains which do not exist, like invalid container names or non-existent subdomains, should return SOA.
///
/// ```
/// # use lxddns::pdns::*;
/// # use lxddns::lxd::*;
/// # use serde_json::{from_value, json};
/// for qname in
///     [ "_container.example.com"
///     , "_acme-challenge._container.example.com"
///     , "fictional.container.example.com"
///     , "fictional._container.example.com"
///     , "fictional._acme-challenge.container.example.com"
///     , "fictional._acme-challenge._container.example.com"
///     ]
/// {
///     for qtype in
///         [ "SOA"
///         , "ANY"
///         , "AAAA"
///         ]
///     {
///         let response = from_value::<QueryParameters>(json!(
///         {
///             "qname": qname,
///             "qtype": qtype,
///             "zone_id": 0,
///         })).unwrap().type_for_domain("example.com");
///
///         assert_eq!(response, LookupType::Dumb
///         {
///             //response: DumbResponse::Soa, NOSOA
///             response: DumbResponse::Nxdomain,
///         }, "wrong response for {} ({})", qname, qtype);
///     }
/// }
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

					if self.qtype.eq("SOA")
					{
						debug!("[queryparameters][type_for_domain][{}][{}] omitting soa on acme for valid container", self.qname(), self.qtype());

						LookupType::Dumb
						{
							response: DumbResponse::Nxdomain,
						}
					}
					else
					{
						debug!("[queryparameters][type_for_domain][{}][{}] using NS on acme for valid container", self.qname(), self.qtype());

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
						}
					}
					else
					{
						debug!("[queryparameters][type_for_domain][{}][{}] is not aaaa-ish", self.qname(), self.qtype());

						LookupType::Dumb
						{
							response: DumbResponse::Nxdomain,
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
			// FIXME: this ttl needs to be configurable
			ttl: 256,
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
			// FIXME: this ttl needs to be configurable
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

