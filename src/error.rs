pub(crate) use ::
{
	thiserror::
	{
		Error,
	},
	anyhow::
	{
		bail,
		Context,
		Result,
	},
	log::
	{
		trace,
		debug,
		info,
		warn,
		error,
	}
};

#[derive(Error,Debug)]
pub enum Error
{
	#[error("command line parsing failure")]
	CommandLineParsing(#[from] ::clap::Error),
	#[error("general io error")]
	Io(#[from] ::std::io::Error),
	#[error("number parsing error")]
	NumberParsing(#[from] ::std::num::ParseIntError),
	#[cfg(feature = "amqp")]
	#[error("lapin (AMQP) error")]
	Lapin(#[from] ::lapin::Error),
	#[cfg(feature = "http")]
	#[error("reqwest error")]
	Reqwest(#[from] ::reqwest::Error),
	#[cfg(feature = "http")]
	#[error("rustls error")]
	Rustls(#[from] ::rustls::Error),
	#[error("responder failed with error")]
	ResponderError,
	#[error("responder closed gracefully")]
	ResponderClosed,
	#[error("unix server failed with error")]
	UnixServerError,
	#[error("unix server closed gracefully")]
	UnixServerClosed,
	#[error("local command output is unparsable")]
	LocalOutput,
	#[error("local resolution via command execution failed with `{0:?}`")]
	LocalExecution(Option<String>),
	#[error("domain name is not safe for resolution: '{0}'")]
	UnsafeName(String),
	#[error("message queue was considered tainted after an error")]
	MessageQueueTaint,
	#[error("invalid provided configuration")]
	InvalidConfiguration,
	#[error("connection to message queue failed")]
	QueueConnectionError,
	#[error("connection to message queue failed")]
	AcknowledgementError,
	#[error("correlation id was reused")]
	DuplicateCorrelationId,
	#[error("http server failed with error")]
	HttpServerError,
	#[error("http request failed with error")]
	HttpRequestError,
}

