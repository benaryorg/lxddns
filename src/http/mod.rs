mod responder;
mod pipe;
mod unix;
mod query;

pub use responder::Responder;
pub use unix::Unix;
pub use pipe::Pipe;

#[derive(Clone,Eq,PartialEq,Ord,PartialOrd,Hash,Debug,serde::Serialize,serde::Deserialize)]
pub enum ApiResponse
{
	V1(ApiResponseV1),
}

#[derive(Clone,Eq,PartialEq,Ord,PartialOrd,Hash,Debug,serde::Serialize,serde::Deserialize)]
pub enum ApiResponseV1
{
	NoMatch,
	AnyMatch(Vec<std::net::Ipv6Addr>),
}

