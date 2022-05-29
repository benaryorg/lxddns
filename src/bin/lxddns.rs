#[rustfmt::skip]
use ::
{
	lxddns::Server,
	std::time::Duration,
	async_std::
	{
		fs::remove_file,
		task,
	},
	clap::Parser,
	log::
	{
		error,
		info,
	},
};

// check https://github.com/clap-rs/clap/issues/3221 at a later time
/// PowerDNS backend to bridge the gap between DNS and LXD.
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args
{
	/// connection string for the message queue
	#[clap(short, long, value_name = "AMQP_URL", default_value = "amqp://guest:guest@[::1]:5672", env = "LXDDNS_URL")]
	url: String,

	/// loglevel to be used, if not specified uses env_logger's auto-detection
	#[clap(short = 'v', long)]
	loglevel: Option<String>,

	/// hostmaster to announce in SOA (use dot notation including trailing dot as in hostmaster.example.org.)
	#[clap(short, long, value_name = "SOA_HOSTMASTER")]
	hostmaster: String,

	/// domain under which to run (do not forget the trailing dot)
	#[clap(short, long)]
	domain: String,

	/// location of the unix domain socket to be created
	#[clap(short, long, value_name = "SOCKET_PATH",  default_value = "/var/run/lxddns/lxddns.sock")]
	socket: String,

	/// name of queue to be used for query responses; if not specified uses randomly assigned queue name
	#[clap(short, long)]
	queuename: Option<String>,

	/// number of parallel worker threads for message queue responders (0: unlimited)
	#[clap(long, value_name = "THREAD_COUNT", default_value = "2")]
	responder_workers: usize,

	/// number of parallel worker threads for unix domain socket connections (0: unlimited)
	#[clap(long, value_name = "THREAD_COUNT", default_value = "2")]
	unix_workers: usize,
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

	let server = Server::builder()
		.url(args.url)
		.domain(args.domain)
		.hostmaster(args.hostmaster)
		.unixpath(&args.socket)
		.queuename(args.queuename.unwrap_or_else(|| "".to_string()))
		.responder_workers(args.responder_workers)
		.unix_workers(args.unix_workers)
	;

	info!("[main] running server");
	match server.run().await
	{
		Ok(_) => unreachable!(),
		Err(err) =>
		{
			error!("[main] fatal error occured: {}", err);
			for err in err.chain().skip(1)
			{
				error!("[main]  caused by: {}", err);
			}
			error!("[main] restarting all services");
		},
	}
	let _ = remove_file(args.socket).await;
	task::sleep(Duration::from_secs(1)).await;
}

