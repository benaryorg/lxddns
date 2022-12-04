#[rustfmt::skip]
use ::
{
	lxddns::
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
	/// loglevel to be used, if not specified uses env_logger's auto-detection
	#[clap(short = 'v', long, global = true)]
	loglevel: Option<String>,

	#[clap(subcommand)]
	command: Command,
}

#[derive(Parser, Debug)]
enum Command
{
	/// run the responder, allowing container names on this host to resolve
	Responder
	{
		/// connection string for the message queue
		#[clap(short, long, value_name = "AMQP_URL", default_value = "amqp://guest:guest@[::1]:5672", env = "LXDDNS_URL")]
		url: String,

		/// name of queue to be used for query responses; if not specified uses randomly assigned queue name
		#[clap(short, long)]
		queue_name: Option<String>,

		/// number of parallel worker threads for message queue responders (0: unlimited)
		#[clap(long, value_name = "THREAD_COUNT", default_value = "2")]
		responder_workers: usize,
	},

	/// run the remote backend via a stdio pipe for PowerDNS
	Pipe
	{
		/// connection string for the message queue
		#[clap(short, long, value_name = "AMQP_URL", default_value = "amqp://guest:guest@[::1]:5672", env = "LXDDNS_URL")]
		url: String,

		/// hostmaster to announce in SOA (use dot notation including trailing dot as in hostmaster.example.org.)
		#[clap(long, value_name = "SOA_HOSTMASTER")]
		hostmaster: String,

		/// domain under which to run (do not forget the trailing dot)
		#[clap(short, long)]
		domain: String,
	},

	/// run the remote backend via a Unix Domain Socket for PowerDNS
	Unix
	{
		/// connection string for the message queue
		#[clap(short, long, value_name = "AMQP_URL", default_value = "amqp://guest:guest@[::1]:5672", env = "LXDDNS_URL")]
		url: String,

		/// hostmaster to announce in SOA (use dot notation including trailing dot as in hostmaster.example.org.)
		#[clap(long, value_name = "SOA_HOSTMASTER")]
		hostmaster: String,

		/// domain under which to run (do not forget the trailing dot)
		#[clap(short, long)]
		domain: String,

		/// location of the unix domain socket to be created
		#[clap(short, long, value_name = "SOCKET_PATH",  default_value = "/var/run/lxddns/lxddns.sock")]
		socket: String,

		/// number of parallel worker threads for unix domain socket connections (0: unlimited)
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
				.queue_name(queue_name.unwrap_or_else(|| "".to_string()))
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

