use crate::msg::{ReadCtx, WriteCtx};
use std::net::SocketAddr;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc,
};

enum Ctl {}

pub async fn endpoint(addr: SocketAddr, cli_addr: SocketAddr) {
    let (ctl_tx, ctl_rx) = mpsc::channel::<Ctl>(1024);
    tokio::spawn(route(addr, ctl_rx));

    // TODO: Handle cli here
}

struct Endpoint {
    read_ctx: ReadCtx,
    write_ctx: WriteCtx,
}

impl Endpoint {
    fn new() -> Self {
        Endpoint {
            read_ctx: ReadCtx::new(),
            write_ctx: WriteCtx::new(),
        }
    }

    fn should_write(&self) -> bool {
        self.write_ctx.written_size.is_some() || self.write_ctx.msgs.len() != 0
    }

    fn handle_msgs(&self) {
        // TODO
    }
}

async fn route(addr: SocketAddr, ctl_rx: mpsc::Receiver<Ctl>) {
    let mut ep = Endpoint::new();
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
                if let Err(e) = ep.read_ctx.handle_read(&mut readhalf) {
                    println!("Failed to handle read: {}", e);
                    let (conn, _) = accept(&mut listener).await;
                    let (rh, wh) = conn.into_split();
                    readhalf = rh;
                    writehalf = wh;
                }
                ep.handle_msgs();
            }
            // Write to Hole.
            r = writehalf.writable(), if ep.should_write() => {
                if let Err(e) = ep.write_ctx.handle_write(&mut writehalf) {
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

async fn accept(listener: &mut TcpListener) -> (TcpStream, SocketAddr) {
    match listener.accept().await {
        Ok(conn) => conn,
        Err(e) => {
            panic!("Failed to accept: {}", e);
        }
    }
}
