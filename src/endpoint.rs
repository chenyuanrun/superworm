use crate::msg::{Msg, MsgCtx};
use std::net::SocketAddr;
use tokio::{net::{TcpListener, TcpStream}, sync::mpsc::{self, Sender}};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

#[derive(Serialize, Deserialize)]
enum Ctl {}

#[derive(Serialize, Deserialize)]
enum CtlRsp {}

type CtlChanMsg = (Ctl, oneshot::Sender<CtlRsp>);

pub async fn endpoint(addr: SocketAddr, cli_addr: SocketAddr) {
    let (ctl_tx, ctl_rx) = mpsc::channel::<CtlChanMsg>(1024);
    tokio::spawn(route(addr, ctl_rx));
    handle_cli(ctl_tx, cli_addr).await;
}

async fn handle_cli(tx: Sender<CtlChanMsg>, cli_addr: SocketAddr) {
    // Listen to cli addr
    let mut msg_ctx = MsgCtx::<Ctl, CtlRsp>::new();
    let mut listener = match TcpListener::bind(&cli_addr).await {
        Ok(r) => r,
        Err(e) => {
            panic!("Failed to bind to {}: {}", cli_addr, e);
        }
    };
    let (conn, _) = accept(&mut listener).await;
    let (mut readhalf, mut writehalf) = conn.into_split();

    loop {
        tokio::select! {
            // Read from cli
            _ = readhalf.readable() => {
                if let Err(e) = msg_ctx.handle_read(&mut readhalf) {
                    println!("Failed to handle read: {}", e);
                    let (conn, _) = accept(&mut listener).await;
                    let (rh, wh) = conn.into_split();
                    readhalf = rh;
                    writehalf = wh;
                }
                // Send Ctl to router and wait for CtlRsp
                loop {
                    if let Some(ctl) = msg_ctx.pop_rx_msg() {
                        let (oneshot_tx, oneshot_rx) = oneshot::channel();
                        if let Err(e) = tx.send((ctl, oneshot_tx)).await {
                            panic!("Ctl receive closed: {}", e);
                        }
                        let rsp = match oneshot_rx.await {
                            Ok(rsp) => rsp,
                            Err(e) => {
                                panic!("Expect a CtlRsp: {}", e);
                            }
                        };
                        msg_ctx.queue_tx_msg(rsp);
                    } else {
                        break;
                    }
                }
            }
            // Write to cli
            _ = writehalf.writable(), if msg_ctx.need_to_write() => {
                if let Err(e) = msg_ctx.handle_write(&mut writehalf) {
                    println!("Failed to handle write: {}", e);
                    let (conn, _) = accept(&mut listener).await;
                    let (rh, wh) = conn.into_split();
                    readhalf = rh;
                    writehalf = wh;
                }
            }
        }
    }
}

struct Endpoint {
    msg_ctx: MsgCtx<Msg, Msg>,
}

impl Endpoint {
    fn new() -> Self {
        Endpoint {
            msg_ctx: MsgCtx::new(),
        }
    }

    async fn handle_msgs(&mut self) {
        // TODO
    }

    async fn handle_ctl(&mut self, ctl: Ctl, oneshot_tx: oneshot::Sender<CtlRsp>) {
        // TODO
    }
}

async fn route(addr: SocketAddr, mut ctl_rx: mpsc::Receiver<CtlChanMsg>) {
    let mut ep = Endpoint::new();
    let mut ctl_rx_closed = false;
    // Listen to addr.
    let mut listener = match TcpListener::bind(&addr).await {
        Ok(r) => r,
        Err(e) => {
            panic!("Failed to bind to {}: {}", addr, e);
        }
    };
    let (conn, _) = accept(&mut listener).await;
    let (mut readhalf, mut writehalf) = conn.into_split();

    loop {
        tokio::select! {
            // Read from Hole.
            r = readhalf.readable() => {
                if let Err(e) = ep.msg_ctx.handle_read(&mut readhalf) {
                    println!("Failed to handle read: {}", e);
                    let (conn, _) = accept(&mut listener).await;
                    let (rh, wh) = conn.into_split();
                    readhalf = rh;
                    writehalf = wh;
                }
                ep.handle_msgs().await;
            }
            // Write to Hole.
            r = writehalf.writable(), if ep.msg_ctx.need_to_write() => {
                if let Err(e) = ep.msg_ctx.handle_write(&mut writehalf) {
                    println!("Failed to handle write: {}", e);
                    let (conn, _) = accept(&mut listener).await;
                    let (rh, wh) = conn.into_split();
                    readhalf = rh;
                    writehalf = wh;
                }
            }
            // Handle Ctl from cli
            r = ctl_rx.recv(), if !ctl_rx_closed => {
                if let Some((ctl, oneshot_tx)) = r {
                    // Handle ctl and then response.
                    ep.handle_ctl(ctl, oneshot_tx).await;
                } else {
                    println!("ctl_rx closed");
                    ctl_rx_closed = true;
                }
            }
        }
    }
}

async fn accept(listener: &mut TcpListener) -> (TcpStream, SocketAddr) {
    match listener.accept().await {
        Ok(conn) => conn,
        Err(e) => {
            panic!("Failed to accept: {}", e);
        }
    }
}
