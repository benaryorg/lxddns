use ::error_chain::error_chain;

error_chain!
{
	links
	{
	}

	foreign_links
	{
		CommandLineParsing(::clap::Error);
		Io(::std::io::Error);
		NumberParsing(::std::num::ParseIntError);
		Lapin(::lapin::Error);
	}

	errors
	{
		LocalOutput
		{
			description("local command output unparsable")
			display("local command produced output that could not be parsed")
		}
		LocalExecution(error: Option<String>)
		{
			description("command execution failed")
			display("local resolution via command execution failed {}",
				error.clone().map(|err| format!("with: '{}'",err)).unwrap_or("without error".to_string())
			)
		}
		UnsafeName(name: String)
		{
			description("unsafe domain name used")
			display("domain name is not safe for resolution: '{}'", name)
		}
	}
}

