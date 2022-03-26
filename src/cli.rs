use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

use crate::{
    endpoint::{Ctl, CtlRsp},
    msg::MsgCtx,
};
use clap::{Args, Subcommand};
use tokio::net::TcpStream;

#[derive(Args)]
pub struct Cli {
    #[clap(long)]
    cli_addr: SocketAddr,

    #[clap(subcommand)]
    action: Action,
}

#[derive(Subcommand, Serialize, Deserialize)]
pub enum Action {
    #[clap(name = "mapadd")]
    MapAdd {
        #[clap(long)]
        lst_addr: SocketAddr,
        #[clap(long)]
        dst_addr: SocketAddr,
    },
    #[clap(name = "maprm")]
    MapRm {
        #[clap(long)]
        lst_addr: SocketAddr,
        #[clap(long)]
        dst_addr: SocketAddr,
    },
    #[clap(name = "mapls")]
    MapLs,
}

pub(crate) async fn cli(arg: Cli) {
    // Connect to endpoint
    let conn = match TcpStream::connect(arg.cli_addr).await {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("Failed to connect to {}: {}", arg.cli_addr, e);
            return;
        }
    };
    let (mut readhalf, mut writehalf) = conn.into_split();
    let mut msg_ctx = MsgCtx::<CtlRsp, Ctl>::new();
    let ctl = Ctl::Act(arg.action);
    // Send Ctl to endpoint
    if let Err(e) = msg_ctx.write(&mut writehalf, ctl).await {
        eprintln!("Failed to write to {}: {}", arg.cli_addr, e);
        return;
    }
    // Wait result from endpoint
    let rsp = match msg_ctx.read(&mut readhalf).await {
        Ok(rsp) => rsp,
        Err(e) => {
            eprintln!("Failed to read from {}: {}", arg.cli_addr, e);
            return;
        }
    };
    // Now print the response.
    println!("{}", rsp);
}
