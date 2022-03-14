use crate::msg::{Msg, ReadCtx, WriteCtx};
use std::net::SocketAddr;
use std::panic;
use tokio::net::TcpStream;
use tokio::sync::mpsc::{self, Permit};
use tokio::time::{sleep, Duration};

pub async fn hole(eps: Vec<SocketAddr>) {
    if eps.len() != 2 {
        println!("Expect 2 ep, found {}", eps.len());
        return;
    }
    let (tx1, rx1) = mpsc::channel(1024);
    let (tx2, rx2) = mpsc::channel(1024);

    tokio::spawn(route(eps[0].clone(), tx1, rx2));
    tokio::spawn(route(eps[1].clone(), tx2, rx1));

    loop {
        sleep(Duration::from_secs(1)).await;
        println!("tick");
        // TODO: exit gracefully.
    }
}

struct Hole {
    read_ctx: ReadCtx,
    write_ctx: WriteCtx,
}

impl Hole {
    fn new() -> Self {
        Hole {
            read_ctx: ReadCtx::new(),
            write_ctx: WriteCtx::new(),
        }
    }

    fn handle_msg_recv(&mut self, msg: Msg) {
        self.write_ctx.msgs.push_back(msg);
    }

    fn handle_msg_send(&mut self, permit: Permit<'_, Msg>) {
        if let Some(msg) = self.read_ctx.msgs.pop_front() {
            permit.send(msg);
        }
    }

    fn should_write(&self) -> bool {
        self.write_ctx.written_size.is_some() || self.write_ctx.msgs.len() != 0
    }

    fn should_send(&self) -> bool {
        self.read_ctx.msgs.len() != 0
    }
}

async fn route(ep: SocketAddr, tx: mpsc::Sender<Msg>, mut rx: mpsc::Receiver<Msg>) {
    let mut hole = Hole::new();
    // Conntect to endpoint.
    let (mut readhalf, mut writehalf) = connect(ep).await.into_split();

    loop {
        tokio::select! {
            // Read from socket.
            r = readhalf.readable() => {
                if let Err(e) = hole.read_ctx.handle_read(&mut readhalf) {
                    println!{"Failed to handle read from {}: {}", ep, e};
                    let (rh, wh) = connect(ep).await.into_split();
                    readhalf = rh;
                    writehalf = wh;
                }
            }
            // Write to socket.
            r = writehalf.writable(), if hole.should_write() => {
                if let Err(e) = hole.write_ctx.handle_write(&mut writehalf) {
                    println!{"Failed to handle write from {}: {}", ep, e};
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
            r = tx.reserve(), if hole.should_send() => {
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
                println!("Failed to connect to {}: {}", addr, e);
                // Try again in a second.
                sleep(Duration::from_secs(1)).await;
            }
        }
    }
}
