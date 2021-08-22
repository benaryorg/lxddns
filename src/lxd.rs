use crate::error::*;

use super::
{
	Deserialize,
	FromStr,
	Getters,
	HashMap,
	static_regex,
};

#[derive(Getters,Deserialize,Clone,Eq,PartialEq,Hash,Debug)]
pub struct CpuState
{
	usage: i128,
}

#[derive(Getters,Deserialize,Clone,Eq,PartialEq,Hash,Debug)]
pub struct DiskState
{
	#[get = "pub"]
	usage: i128,
}

#[derive(Getters,Deserialize,Clone,Eq,PartialEq,Hash,Debug)]
pub struct MemoryState
{
	#[get = "pub"]
	swap_usage: i128,
	#[get = "pub"]
	swap_usage_peak: i128,
	#[get = "pub"]
	usage: i128,
	#[get = "pub"]
	usage_peak: i128,
}

#[derive(Deserialize,Clone,Eq,PartialEq,Hash,Debug)]
pub enum AddressFamily
{
	#[serde(rename = "inet6")]
	Inet6,
	#[serde(rename = "inet")]
	Inet,
}

#[derive(Deserialize,Clone,Eq,PartialEq,Hash,Debug)]
pub enum AddressScope
{
	#[serde(rename = "local")]
	Local,
	#[serde(rename = "global")]
	Global,
	#[serde(rename = "link")]
	Link,
}

#[derive(Getters,Deserialize,Clone,Eq,PartialEq,Hash,Debug)]
pub struct Address
{
	#[get = "pub"]
	address: String,
	#[get = "pub"]
	family: AddressFamily,
	#[get = "pub"]
	scope: AddressScope,
	#[get = "pub"]
	netmask: String,
}

#[derive(Getters,Deserialize,Clone,Eq,PartialEq,Hash,Debug)]
pub struct NetCounters
{
	#[get = "pub"]
	bytes_received: i128,
	#[get = "pub"]
	bytes_sent: i128,
	#[get = "pub"]
	packets_received: i128,
	#[get = "pub"]
	packets_sent: i128,
}

#[derive(Getters,Deserialize,Clone,Eq,PartialEq,Hash,Debug)]
pub struct NetState
{
	#[get = "pub"]
	addresses: Vec<Address>,
	#[get = "pub"]
	counters: NetCounters,
	#[get = "pub"]
	host_name: String,
	#[get = "pub"]
	hwaddr: String,
	#[get = "pub"]
	mtu: isize,
	#[get = "pub"]
	state: String,
	// too lazy to find a workaround
	// type: String,
}

#[derive(Getters,Deserialize,Clone,Eq,PartialEq,Debug)]
pub struct ContainerState
{
	#[get = "pub"]
	pid: isize,
	#[get = "pub"]
	processes: isize,
	// probably breaks if enum
	#[get = "pub"]
	status: String,
	#[get = "pub"]
	status_code: isize,
	#[get = "pub"]
	cpu: CpuState,
	#[get = "pub"]
	disk: HashMap<String,DiskState>,
	#[get = "pub"]
	network: HashMap<String,NetState>,
	#[get = "pub"]
	memory: MemoryState,
}

#[derive(Getters,Hash,Clone,Eq,Ord,PartialEq,PartialOrd,Debug)]
pub struct ContainerName
{
	#[get = "pub"]
	name: String
}

impl AsRef<str> for ContainerName
{
	fn as_ref(&self) -> &str
	{
		self.name()
	}
}

impl FromStr for ContainerName
{
	type Err = crate::error::Error;

	fn from_str(name: &str) -> std::result::Result<Self,Self::Err>
	{
		if !static_regex!(r"\A[-a-z0-9]+\z").is_match(&name)
		{
			Err(Error::UnsafeName(name.to_string()).into())
		}
		else
		{
			Ok(Self
			{
				name: name.to_string()
			})
		}
	}
}

