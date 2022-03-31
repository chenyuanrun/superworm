use crate::msg::{Msg, MsgCtx};
use log::error;
use std::net::SocketAddr;
use std::panic;
use tokio::net::TcpStream;
use tokio::sync::mpsc::{self, Permit};
use tokio::time::{sleep, Duration};

pub async fn hole(eps: Vec<SocketAddr>) {
    if eps.len() != 2 {
        error!("{}:{} Expect 2 ep, found {}", file!(), line!(), eps.len());
        return;
    }
    let (tx1, rx1) = mpsc::channel(1024);
    let (tx2, rx2) = mpsc::channel(1024);

    tokio::spawn(route(eps[0].clone(), tx1, rx2));
    tokio::spawn(route(eps[1].clone(), tx2, rx1));

    loop {
        sleep(Duration::from_secs(1)).await;
    }
}

struct Hole {
    msg_ctx: MsgCtx<Msg, Msg>,
}

impl Hole {
    fn new() -> Self {
        Hole {
            msg_ctx: MsgCtx::new(),
        }
    }

    fn handle_msg_recv(&mut self, msg: Msg) {
        self.msg_ctx.queue_tx_msg(msg);
    }

    fn handle_msg_send(&mut self, permit: Permit<'_, Msg>) {
        if let Some(msg) = self.msg_ctx.pop_rx_msg() {
            permit.send(msg);
        }
    }
}

async fn route(ep: SocketAddr, tx: mpsc::Sender<Msg>, mut rx: mpsc::Receiver<Msg>) {
    let mut hole = Hole::new();
    // Conntect to endpoint.
    let (mut readhalf, mut writehalf) = connect(ep).await.into_split();

    loop {
        tokio::select! {
            // Read from socket.
            _ = readhalf.readable() => {
                if let Err(e) = hole.msg_ctx.handle_read(&mut readhalf) {
                    if e.kind() == std::io::ErrorKind::WouldBlock {
                        continue;
                    }
                    error!{"{}:{} Failed to handle read from {}: {}", file!(), line!(), ep, e};
                    hole.msg_ctx.reset_read();
                    let (rh, wh) = connect(ep).await.into_split();
                    readhalf = rh;
                    writehalf = wh;
                }
            }
            // Write to socket.
            _ = writehalf.writable(), if hole.msg_ctx.need_to_write() => {
                if let Err(e) = hole.msg_ctx.handle_write(&mut writehalf) {
                    if e.kind() == std::io::ErrorKind::WouldBlock {
                        continue;
                    }
                    error!{"{}:{} Failed to handle write from {}: {}", file!(), line!(), ep, e};
                    hole.msg_ctx.reset_write();
                    let (rh, wh) = connect(ep).await.into_split();
                    readhalf = rh;
                    writehalf = wh;
                }
            }
            // Read from channel.
            r = rx.recv() => {
                match r {
                    Some(msg) => {
                        hole.handle_msg_recv(msg);
                    },
                    None => {
                        panic!{"Receiver closed for {}", ep};
                    },
                }
            }
            // Write to channel.
            r = tx.reserve(), if hole.msg_ctx.have_rx_msg() => {
                match r {
                    Ok(permit) => {
                        hole.handle_msg_send(permit);
                    },
                    Err(e) => {
                        panic!{"Sender closed for {}: {}", ep, e};
                    }
                }
            }
        }
    }
}

async fn connect(addr: SocketAddr) -> TcpStream {
    loop {
        match TcpStream::connect(addr).await {
            Ok(conn) => {
                return conn;
            }
            Err(e) => {
                error!(
                    "{}:{} Failed to connect to {}: {}",
                    file!(),
                    line!(),
                    addr,
                    e
                );
                // Try again in a second.
                sleep(Duration::from_secs(1)).await;
            }
        }
    }
}
