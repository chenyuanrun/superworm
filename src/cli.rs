use log::{debug, error};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, path::PathBuf};

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

#[derive(Subcommand, Serialize, Deserialize, Debug)]
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
    #[clap(name = "mapload")]
    MapLoad {
        #[clap(long)]
        map_file: PathBuf,
    },
    #[clap(name = "mapdump")]
    MapDump,
}

pub(crate) async fn cli(arg: Cli) {
    // Connect to endpoint
    let conn = match TcpStream::connect(arg.cli_addr).await {
        Ok(conn) => conn,
        Err(e) => {
            error!(
                "{}:{} Failed to connect to {}: {}",
                file!(),
                line!(),
                arg.cli_addr,
                e
            );
            return;
        }
    };
    let (mut readhalf, mut writehalf) = conn.into_split();
    let mut msg_ctx = MsgCtx::<CtlRsp, Ctl>::new();
    let ctl = Ctl::Act(arg.action);
    debug!("{}:{} Send ctl to endpoint: {:?}", file!(), line!(), ctl);
    // Send Ctl to endpoint
    if let Err(e) = msg_ctx.write(&mut writehalf, ctl).await {
        error!(
            "{}:{} Failed to write to {}: {}",
            file!(),
            line!(),
            arg.cli_addr,
            e
        );
        return;
    }
    debug!("{}:{} Waiting for ctl rsp", file!(), line!());
    // Wait result from endpoint
    let rsp = match msg_ctx.read(&mut readhalf).await {
        Ok(rsp) => rsp,
        Err(e) => {
            error!(
                "{}:{} Failed to read from {}: {}",
                file!(),
                line!(),
                arg.cli_addr,
                e
            );
            return;
        }
    };
    debug!("{}:{} Received ctl rsp: {:?}", file!(), line!(), rsp);
    // Now print the response.
    println!("{}", rsp);
}
