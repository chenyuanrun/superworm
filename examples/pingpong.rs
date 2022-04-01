use clap::{Parser, Subcommand};
use log::{debug, error, info};
use std::{io::Write, net::SocketAddr};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

#[derive(Parser)]
#[clap(name = "pingpong")]
struct _Args {
    #[clap(subcommand)]
    subcmd: Subcmd,
}

#[derive(Subcommand)]
enum Subcmd {
    #[clap(name = "ping")]
    Ping {
        #[clap(long)]
        addr: SocketAddr,
    },
    #[clap(name = "pong")]
    Pong {
        #[clap(long)]
        addr: SocketAddr,
    },
}

#[tokio::main]
async fn main() {
    env_logger::init();

    match _Args::parse().subcmd {
        Subcmd::Ping { addr } => ping(addr).await,
        Subcmd::Pong { addr } => pong(addr).await,
    }
}

async fn ping(addr: SocketAddr) {
    let mut conn = match TcpStream::connect(addr.clone()).await {
        Ok(conn) => conn,
        Err(e) => {
            error!("Failed to connect to {}: {}", addr, e);
            return;
        }
    };
    info!("Connected to {}", addr);

    loop {
        let mut input = String::new();
        print!(":: ");
        let _ = std::io::stdout().flush();
        match std::io::stdin().read_line(&mut input) {
            Ok(s) => {
                debug!("Read {} byte from stdin", s);
                if s == 0 {
                    info!("Reach eof");
                    return;
                }
            }
            Err(e) => {
                error!("Failed to read from stdin: {}", e);
                return;
            }
        }
        if let Err(e) = conn.write_all(input.as_bytes()).await {
            error!("Failed to write to {}: {}", addr, e);
            return;
        }

        let mut buf: Vec<u8> = Vec::new();
        match conn.read_buf(&mut buf).await {
            Ok(s) => {
                debug!("Read {} byte from {}", s, addr);
                if s == 0 {
                    info!("Peer {} closed", addr);
                }
            }
            Err(e) => {
                error!("Failed to read from {}: {}", addr, e);
                return;
            }
        }
        print!(">> {}", std::str::from_utf8(&buf[..]).unwrap());
        let _ = std::io::stdout().flush();
    }
}

async fn pong(addr: SocketAddr) {
    let lst = match TcpListener::bind(addr.clone()).await {
        Ok(lst) => lst,
        Err(e) => {
            error!("Failed to bind {}:{}", addr, e);
            return;
        }
    };
    info!("Bind to addr {}", addr);

    let (mut conn, local_addr) = match lst.accept().await {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to accept {}: {}", addr, e);
            return;
        }
    };
    info!("Accept connection from {}", local_addr);

    loop {
        let mut buf: Vec<u8> = Vec::new();
        match conn.read_buf(&mut buf).await {
            Ok(s) => {
                debug!("Read {} byte from {}", s, local_addr);
                if s == 0 {
                    info!("Peer {} closed", local_addr);
                    return;
                }
            }
            Err(e) => {
                error!("Failed to read from {}: {}", local_addr, e);
                return;
            }
        };
        if let Err(e) = conn.write_all(&buf[..]).await {
            error!("Failed to write to {}: {}", local_addr, e);
            return;
        }
    }
}
