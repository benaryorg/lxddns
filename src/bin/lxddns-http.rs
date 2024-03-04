// Copyright (C) benaryorg <binary@benary.org>
//
// This software is licensed as described in the file COPYING, which
// you should have received as part of this distribution.
//
// SPDX-License-Identifier: AGPL-3.0-or-later

#[rustfmt::skip]
use ::
{
	lxddns::
	{
		http::
		{
			Responder,
			Pipe,
			Unix,
		},
		pdns::
		{
			TtlConfig,
		},
	},
	clap::
	{
		Parser,
	},
	log::
	{
		info,
		error,
	},
};

// check https://github.com/clap-rs/clap/issues/3221 at a later time
/// PowerDNS backend to bridge the gap between DNS and LXD.
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args
{
	/// Loglevel to be used, if not specified uses env_logger's auto-detection
	#[clap(short = 'v', long, global = true)]
	loglevel: Option<String>,

	#[clap(subcommand)]
	command: Command,
}

#[derive(Parser, Debug)]
enum Command
{
	/// Run the HTTP responder, allowing container names on this host to resolve
	#[clap(alias = "http-responder")]
	Responder
	{
		/// Command name for querying. Set to "incus" if you're running Incus.
		#[clap(short, long, value_name = "COMMAND", default_value = "lxc")]
		command: String,

		/// Maximum number of connections per worker. Additional connections will hang on accept until a connection is freed up.
		#[clap(short = 'm', long, value_name = "MAX_CONNS", default_value = "32")]
		max_connections: usize,

		/// Address-Port pair to bind to for incoming HTTPS traffic.
		#[clap(short = 'b', long, value_name = "HTTP:PORT", default_value = "[::1]:9132", env = "LXDDNS_HTTP_BIND")]
		https_bind: String,

		/// File containing the TLS certificate chain.
		#[clap(short = 'c', long, value_name = "FILE", env = "LXDDNS_HTTP_TLS_CHAIN")]
		tls_chain: String,

		/// File containing the TLS key.
		#[clap(short = 'k', long, value_name = "FILE", env = "LXDDNS_HTTP_TLS_KEY")]
		tls_key: String,
	},

	/// Run the HTTP remote backend via a stdio pipe for PowerDNS
	#[clap(alias = "http-pipe")]
	Pipe
	{
		/// API root of remote instances.
		///
		/// The root for of a remote API with the endpoint `https://example.com/lxddns/v1/resolve` would thus be `https://example.com/lxddns`.
		#[clap(short, long, value_name = "API_ROOT")]
		remote: Vec<String>,

		/// Hostmaster to announce in SOA (use dot notation including trailing dot as in hostmaster.example.org.)
		#[clap(long, value_name = "SOA_HOSTMASTER")]
		hostmaster: String,

		/// Domain under which to run (do not forget the trailing dot)
		#[clap(short, long)]
		domain: String,

		#[command(flatten)]
		ttl_config: TtlConfig,
	},

	/// Run the HTTP remote backend via a Unix Domain Socket for PowerDNS
	#[clap(alias = "http-unix")]
	Unix
	{
		/// API root of remote instances.
		///
		/// The root for of a remote API with the endpoint `https://example.com/lxddns/v1/resolve` would thus be `https://example.com/lxddns`.
		#[clap(short, long, value_name = "API_ROOT")]
		remote: Vec<String>,

		/// Hostmaster to announce in SOA (use dot notation including trailing dot as in hostmaster.example.org.)
		#[clap(long, value_name = "SOA_HOSTMASTER")]
		hostmaster: String,

		/// Domain under which to run (do not forget the trailing dot)
		#[clap(short, long)]
		domain: String,

		/// Location of the unix domain socket to be created
		#[clap(short, long, value_name = "SOCKET_PATH", default_value = "/var/run/lxddns/lxddns.sock")]
		socket: String,

		/// Number of parallel worker threads for unix domain socket connections (0: unlimited)
		#[clap(long, value_name = "THREAD_COUNT", default_value = "2")]
		unix_workers: usize,

		#[command(flatten)]
		ttl_config: TtlConfig,
	},
}

#[tokio::main]
async fn main()
{
	let args = Args::parse();

	if let Some(loglevel) = args.loglevel
	{
		std::env::set_var("RUST_LOG", loglevel);
	}

	env_logger::Builder::new()
		.parse_default_env()
		.format_timestamp(Some(env_logger::TimestampPrecision::Millis))
		.init();

	info!("[main] logging initialised");

	match args.command
	{
		Command::Pipe { ttl_config, remote, hostmaster, domain, } =>
		{
			let pipe = Pipe::builder()
				.remote(remote)
				.domain(domain)
				.ttl_config(ttl_config)
				.hostmaster(hostmaster)
			;

			info!("[main] running http-pipe");
			match pipe.run().await
			{
				Ok(_) => {},
				Err(err) =>
				{
					error!("[main][http-pipe] fatal error occured: {}", err);
					for err in err.chain().skip(1)
					{
						error!("[main][http-pipe]  caused by: {}", err);
					}
					error!("[main][http-pipe] restarting all services");
				},
			}
		},
		Command::Responder { command, https_bind, tls_chain, tls_key, max_connections, } =>
		{
			let responder = Responder::builder()
				.command(command)
				.max_connections(max_connections)
				.https_bind(https_bind)
				.tls_chain(tls_chain)
				.tls_key(tls_key)
			;

			info!("[main] running http-responder");
			match responder.run().await
			{
				Ok(_) => unreachable!(),
				Err(err) =>
				{
					error!("[main][http-responder] fatal error occured: {}", err);
					for err in err.chain().skip(1)
					{
						error!("[main][http-responder]  caused by: {}", err);
					}
					error!("[main][http-responder] restarting all services");
				},
			}
		},
		Command::Unix { ttl_config, remote, domain, hostmaster, socket, unix_workers, } =>
		{
			let unix = Unix::builder()
				.remote(remote)
				.domain(domain)
				.hostmaster(hostmaster)
				.unixpath(socket)
				.ttl_config(ttl_config)
				.unix_workers(unix_workers)
			;

			info!("[main] running unix");
			match unix.run().await
			{
				Ok(_) => {},
				Err(err) =>
				{
					error!("[main][http-unix] fatal error occured: {}", err);
					for err in err.chain().skip(1)
					{
						error!("[main][http-unix]  caused by: {}", err);
					}
					error!("[main][http-unix] restarting all services");
				},
			}
		},
	}
}

