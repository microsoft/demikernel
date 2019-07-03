use std::net::Ipv4Addr;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Ipv4Endpoint {
    pub address: Ipv4Addr,
    pub port: u16,
}
