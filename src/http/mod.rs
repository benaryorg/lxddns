pub mod responder;
pub mod pipe;
pub mod unix;
pub mod query;

#[derive(Clone,Eq,PartialEq,Ord,PartialOrd,Hash,Debug,serde::Serialize,serde::Deserialize)]
pub enum ApiResponse
{
	V1(Option<Vec<std::net::Ipv6Addr>>),
}

