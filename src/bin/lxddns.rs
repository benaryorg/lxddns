#[rustfmt::skip]
use ::
{
	lxddns::Server,
	std::time::Duration,
	async_std::
	{
		fs::remove_file,
		path::Path,
		task,
	},
	clap::
	{
		app_from_crate,
		crate_authors,
		crate_description,
		crate_name,
		crate_version,
		Arg,
	},
	log::
	{
		error,
		info,
	},
};

#[async_std::main]
async fn main()
{
	let matches = app_from_crate!()
        .arg(Arg::with_name("url")
            .short("u")
            .long("url")
            .help("connection string for the message queue")
            .takes_value(true)
            .env("LXDDNS_URL")
            .value_name("AMQP_URL")
            .default_value("amqp://guest:guest@[::1]:5672")
            .multiple(false)
		)
        .arg(Arg::with_name("loglevel")
            .short("v")
            .long("loglevel")
            .help("loglevel to be used, if not specified uses env_logger's auto-detection")
            .takes_value(true)
            .value_name("LOGLEVEL")
            .multiple(false)
		)
        .arg(Arg::with_name("hostmaster")
            .short("h")
            .long("hostmaster")
            .help("hostmaster to announce in SOA (use dot notation including trailing dot as in hostmaster.example.org.)")
            .takes_value(true)
            .value_name("SOA_HOSTMASTER")
            .multiple(false)
            .required(true)
		)
        .arg(Arg::with_name("domain")
            .short("d")
            .long("domain")
            .help("domain under which to run (do not forget the trailing dot)")
            .takes_value(true)
            .value_name("DOMAIN")
            .multiple(false)
            .required(true)
		)
		.arg(Arg::with_name("socket")
            .short("s")
            .long("socket")
            .help("location of the unix domain socket to be created")
            .takes_value(true)
            .value_name("SOCKET_PATH")
            .default_value("/var/run/lxddns/lxddns.sock")
            .multiple(false)
		)
		.arg(Arg::with_name("queuename")
            .short("q")
            .long("queuename")
            .help("name of queue to be used for query responses; empty string is randomly assigned queue name")
            .takes_value(true)
            .value_name("QUEUE_NAME")
            .default_value("")
            .multiple(false)
		)
		.arg(Arg::with_name("responder-workers")
            .long("responder-workers")
            .help("number of parallel worker threads for message queue responders (0: unlimited)")
            .takes_value(true)
            .value_name("THREAD_COUNT")
            .default_value("2")
            .validator(|value| value.as_str().parse::<usize>().map(|_| ()).map_err(|err| format!("{}", err)))
            .multiple(false)
		)
		.arg(Arg::with_name("unix-workers")
            .long("unix-workers")
            .help("number of parallel worker threads for unix domain socket connections (0: unlimited)")
            .takes_value(true)
            .value_name("THREAD_COUNT")
            .default_value("2")
            .validator(|value| value.as_str().parse::<usize>().map(|_| ()).map_err(|err| format!("{}", err)))
            .multiple(false)
		)
		.get_matches();

	if let Some(loglevel) = matches.value_of("loglevel")
	{
		std::env::set_var("RUST_LOG", loglevel);
	}

	env_logger::Builder::new()
		.parse_default_env()
		.format_timestamp(Some(env_logger::TimestampPrecision::Millis))
		.init();

	info!("[main] logging initialised");

	let server = Server::builder()
		.url(matches.value_of("url").unwrap())
		.domain(matches.value_of("domain").unwrap())
		.hostmaster(matches.value_of("hostmaster").unwrap())
		.unixpath(Path::new(matches.value_of("socket").unwrap()))
		.queuename(matches.value_of("queuename").unwrap())
		.responder_workers(matches.value_of("responder-workers").unwrap().parse::<usize>().unwrap())
		.unix_workers(matches.value_of("unix-workers").unwrap().parse::<usize>().unwrap())
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
	let _ = remove_file(matches.value_of("socket").unwrap()).await;
	task::sleep(Duration::from_secs(1)).await;
}

