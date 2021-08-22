#[rustfmt::skip]
use ::
{
	lxddns::run,
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
async fn main() -> !
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
		.get_matches();

	if let Some(loglevel) = matches.value_of("loglevel")
	{
		std::env::set_var("RUST_LOG", loglevel);
	}

	env_logger::init();
	info!("[main] logging initialised");

	let url = matches.value_of("url").unwrap();
	let domain = matches.value_of("domain").unwrap();
	let hostmaster = matches.value_of("hostmaster").unwrap();
	let unixpath = Path::new(matches.value_of("socket").unwrap());

	loop
	{
		info!("[main] running all services");
		match run(&unixpath, &url, &domain, &hostmaster).await
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
		let _ = remove_file(&unixpath).await;
		task::sleep(Duration::from_secs(1)).await;
	}
}