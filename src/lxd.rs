// Copyright (C) benaryorg <binary@benary.org>
//
// This software is licensed as described in the file COPYING, which
// you should have received as part of this distribution.
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::error::*;

use ::
{
	getset::Getters,
	serde::
	{
		Deserialize,
	},
	lazy_regex::regex_is_match,
	tokio::
	{
		process::
		{
			Command,
		},
	},
	std::
	{
		str::FromStr,
		collections::HashMap,
		net::Ipv6Addr,
		process::Stdio,
		time::
		{
			Instant,
		},
	},
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
	network: Option<HashMap<String,NetState>>,
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
		if !regex_is_match!(r"\A[-a-z0-9]+\z", name)
		{
			Err(Error::UnsafeName(name.to_string()))
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

/// Queries the local LXD instance.
/// The local instance is queried by executing an `lxc query` command with *sudo*, where `lxc` is the passed command.
///
/// Values returned are either `Err` if querying failed, `Ok(None)` if the instance was not found locally, or `Ok(vec![])` if the instance was found.
/// The last case includes instances without addresses assigned.
pub async fn local_query(command: &String, name: &ContainerName) -> Result<Option<Vec<Ipv6Addr>>>
{
	trace!("[local_query][{}] starting query", name.as_ref());

	let instant = Instant::now();

	// maybe switch to reqwest some day?

	trace!("[local_query][{}] getting instance list", name.as_ref());
	// first get the list of instances
	let output = Command::new("sudo")
		.arg(command)
		.arg("query")
		.arg("--")
		.arg("/1.0/instances")
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.output()
		.await
		.context(Error::LocalExecution(None))?;

	debug!("[local_query][{}] instance listing ran for {:.3}s", name.as_ref(), instant.elapsed().as_secs_f64());

	trace!("[local_query][{}] validating instance list command output", name.as_ref());
	if !output.status.success()
	{
		let err = String::from_utf8_lossy(&output.stderr);
		bail!(Error::LocalExecution(Some(err.to_string())))
	}

	trace!("[local_query][{}] parsing instance list", name.as_ref());
	let instances: Vec<String> = serde_json::from_slice(&output.stdout).context(Error::LocalOutput)?;

	trace!("[local_query][{}] validating and filtering instance list", name.as_ref());
	let instances = instances.into_iter()
		.filter_map(|instance|
		{
			let instance = match instance.strip_prefix("/1.0/instances/")
			{
				Some(instance) => instance,
				None => return None,
			};

			if name.as_ref().eq(instance)
			{
				trace!("[local_query][{}] exact match", name.as_ref());
				Some((true,instance.to_string()))
			}
			else
			{
				if let Some(remainder) = instance.strip_prefix(name.as_ref())
				{
					if !remainder.contains(|ch: char| !ch.is_ascii_digit())
					{
						trace!("[local_query][{}] prefix match: {}", name.as_ref(), instance);
						Some((false,instance.to_string()))
					}
					else
					{
						trace!("[local_query][{}] prefix does not match: {}", name.as_ref(), instance);
						None
					}
				}
				else
				{
					trace!("[local_query][{}] no match", name.as_ref());
					None
				}
			}
		})
		.collect::<Vec<_>>()
	;

	// this assumes that all matches are either exact or there is only one local instance matching
	// in all cases there will only be one query
	let instance = if let Some((_,instance)) = instances.iter().find(|(exact,_)| *exact)
	{
		Some(instance)
	}
	else
	{
		instances.get(0).map(|(_,instance)| instance)
	};

	let instance = match instance
	{
		Some(instance) =>
		{
			debug!("[local_query][{}] match: {}", name.as_ref(), instance);
			instance
		}
		None =>
		{
			debug!("[local_query][{}] not found", name.as_ref());
			return Ok(None);
		}
	};

	trace!("[local_query][{}] querying state", name.as_ref());
	let output = Command::new("sudo")
		.arg(command)
		.arg("query")
		.arg("--")
		.arg(format!("/1.0/instances/{}/state", instance))
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.output()
		.await
		.context(Error::LocalExecution(None))?;

	debug!("[local_query][{}] query ran for {:.3}s", name.as_ref(), instant.elapsed().as_secs_f64());

	if !output.status.success()
	{
		if &output.stderr == b"Error: not found\n"
		{
			trace!("[local_query][{}] \"not found\"", name.as_ref());
			return Ok(None);
		}
		let err = String::from_utf8_lossy(&output.stderr);
		bail!(Error::LocalExecution(Some(err.to_string())))
	}

	trace!("[local_query][{}] got response", name.as_ref());
	let state: ContainerState = serde_json::from_slice(&output.stdout).context(Error::LocalOutput)?;

	if state.status() != "Running"
	{
		trace!("[local_query][{}] not running", name.as_ref());
		return Ok(None);
	}

	let network = match state.network()
	{
		Some(network) => network,
		None =>
		{
			debug!("[local_query][{}] network is null despite container running, returning None", name.as_ref());
			return Ok(None);
		},
	};

	let addresses = network
		.values()
		.flat_map(|net| net.addresses().iter())
		.filter(|address| address.scope() == &AddressScope::Global && address.family() == &AddressFamily::Inet6)
		.filter_map(|address| address.address().parse::<Ipv6Addr>().ok())
		.collect::<Vec<_>>();

	trace!("[local_query][{}] result: {:?}", name.as_ref(), addresses);

	Ok(Some(addresses))
}

