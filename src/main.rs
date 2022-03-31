use clap::{Parser, Subcommand};
use std::net::SocketAddr;

mod cli;
mod endpoint;
mod hole;
mod msg;

#[derive(Parser)]
#[clap(name = "Superworm")]
struct _Args {
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
    #[clap(name = "cli")]
    Cli(cli::Cli),
}

#[tokio::main]
async fn main() {
    let args = _Args::parse();
    match args._type {
        Type::Hole { eps } => hole::hole(eps).await,
        Type::Endpoint { addr, cli_addr } => endpoint::endpoint(addr, cli_addr).await,
        Type::Cli(cli) => cli::cli(cli).await,
    }
}
