use clap::{Parser, Subcommand};
use std::net::SocketAddr;

mod endpoint;
mod hole;
mod msg;

#[derive(Parser)]
#[clap(name = "Superworm")]
struct Args {
    #[clap(subcommand)]
    _type: Type,
}

#[derive(Subcommand)]
enum Type {
    #[clap(name = "hole")]
    Hole {
        #[clap(long)]
        eps: Vec<SocketAddr>,
    },
    #[clap(name = "endpoint")]
    Endpoint {
        #[clap(long)]
        addr: SocketAddr,
        #[clap(long)]
        cli_addr: SocketAddr,
    },
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    match args._type {
        Type::Hole { eps } => hole::hole(eps).await,
        Type::Endpoint { addr, cli_addr } => endpoint::endpoint(addr, cli_addr).await,
    }
}
