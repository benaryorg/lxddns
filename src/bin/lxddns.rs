#[rustfmt::skip]
use ::
{
	lxddns::
	{
		AmqpResponder,
		AmqpPipe,
		AmqpUnix,
		HttpResponder,
		HttpPipe,
		HttpUnix,
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
	HttpResponder
	{
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
	HttpPipe
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
	},

	/// Run the HTTP remote backend via a Unix Domain Socket for PowerDNS
	HttpUnix
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
		#[clap(short, long, value_name = "SOCKET_PATH",  default_value = "/var/run/lxddns/lxddns.sock")]
		socket: String,

		/// Number of parallel worker threads for unix domain socket connections (0: unlimited)
		#[clap(long, value_name = "THREAD_COUNT", default_value = "2")]
		unix_workers: usize,
	},

	/// Run the AMQP (e.g. RabbitMQ) responder, allowing container names on this host to resolve
	#[clap(alias = "responder")]
	AmqpResponder
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
	#[clap(alias = "pipe")]
	AmqpPipe
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
	#[clap(alias = "unix")]
	AmqpUnix
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
		#[clap(short, long, value_name = "SOCKET_PATH",  default_value = "/var/run/lxddns/lxddns.sock")]
		socket: String,

		/// Number of parallel worker threads for unix domain socket connections (0: unlimited)
		#[clap(long, value_name = "THREAD_COUNT", default_value = "2")]
		unix_workers: usize,
	},
}

#[async_std::main]
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
		Command::AmqpPipe { url, domain, hostmaster, } =>
		{
			let pipe = AmqpPipe::builder()
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
		Command::AmqpUnix { url, domain, hostmaster, socket, unix_workers, } =>
		{
			let unix = AmqpUnix::builder()
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
		Command::AmqpResponder { url, queue_name, responder_workers, } =>
		{
			let responder = AmqpResponder::builder()
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
		Command::HttpPipe { remote, hostmaster, domain, } =>
		{
			let pipe = HttpPipe::builder()
				.remote(remote)
				.domain(domain)
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
		Command::HttpResponder { https_bind, tls_chain, tls_key, } =>
		{
			let responder = HttpResponder::builder()
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
		Command::HttpUnix { remote, domain, hostmaster, socket, unix_workers, } =>
		{
			let unix = HttpUnix::builder()
				.remote(remote)
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

