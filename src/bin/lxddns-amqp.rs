// Copyright (C) benaryorg <binary@benary.org>
//
// This software is licensed as described in the file COPYING, which
// you should have received as part of this distribution.
//
// SPDX-License-Identifier: AGPL-3.0-or-later

#[rustfmt::skip]
use ::
{
	lxddns::amqp::
	{
		Responder,
		Pipe,
		Unix,
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
	/// Run the AMQP (e.g. RabbitMQ) responder, allowing container names on this host to resolve
	#[clap(alias = "amqp-responder")]
	Responder
	{
		/// Connection string for the message queue
		#[clap(short, long, value_name = "AMQP_URL", default_value = "amqp://guest:guest@[::1]:5672", env = "LXDDNS_URL")]
		url: String,

		/// Name of queue to be used for query responses; if not specified uses randomly assigned queue name
		#[clap(short, long)]
		queue_name: Option<String>,

		/// Number of parallel worker threads for message queue responders (0: unlimited)
		#[clap(long, value_name = "THREAD_COUNT", default_value = "2")]
		responder_workers: usize,
	},

	/// Run the AMQP remote backend via a stdio pipe for PowerDNS
	#[clap(alias = "amqp-pipe")]
	Pipe
	{
		/// Connection string for the message queue
		#[clap(short, long, value_name = "AMQP_URL", default_value = "amqp://guest:guest@[::1]:5672", env = "LXDDNS_URL")]
		url: String,

		/// Hostmaster to announce in SOA (use dot notation including trailing dot as in hostmaster.example.org.)
		#[clap(long, value_name = "SOA_HOSTMASTER")]
		hostmaster: String,

		/// Domain under which to run (do not forget the trailing dot)
		#[clap(short, long)]
		domain: String,
	},

	/// Run the AMQP remote backend via a Unix Domain Socket for PowerDNS
	#[clap(alias = "amqp-unix")]
	Unix
	{
		/// Connection string for the message queue
		#[clap(short, long, value_name = "AMQP_URL", default_value = "amqp://guest:guest@[::1]:5672", env = "LXDDNS_URL")]
		url: String,

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
		Command::Pipe { url, domain, hostmaster, } =>
		{
			let pipe = Pipe::builder()
				.url(url)
				.domain(domain)
				.hostmaster(hostmaster)
			;

			info!("[main] running pipe");
			match pipe.run().await
			{
				Ok(_) => {},
				Err(err) =>
				{
					error!("[main][pipe] fatal error occured: {}", err);
					for err in err.chain().skip(1)
					{
						error!("[main][pipe]  caused by: {}", err);
					}
					error!("[main][pipe] restarting all services");
				},
			}
		},
		Command::Unix { url, domain, hostmaster, socket, unix_workers, } =>
		{
			let unix = Unix::builder()
				.url(url)
				.domain(domain)
				.hostmaster(hostmaster)
				.unixpath(socket)
				.unix_workers(unix_workers)
			;

			info!("[main] running unix");
			match unix.run().await
			{
				Ok(_) => {},
				Err(err) =>
				{
					error!("[main][unix] fatal error occured: {}", err);
					for err in err.chain().skip(1)
					{
						error!("[main][unix]  caused by: {}", err);
					}
					error!("[main][unix] restarting all services");
				},
			}
		},
		Command::Responder { url, queue_name, responder_workers, } =>
		{
			let responder = Responder::builder()
				.url(url)
				.queue_name(queue_name.unwrap_or_default())
				.responder_workers(responder_workers)
			;

			info!("[main] running responder");
			match responder.run().await
			{
				Ok(_) => unreachable!(),
				Err(err) =>
				{
					error!("[main][responder] fatal error occured: {}", err);
					for err in err.chain().skip(1)
					{
						error!("[main][responder]  caused by: {}", err);
					}
					error!("[main][responder] restarting all services");
				},
			}
		},
	}
}

