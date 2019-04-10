#![feature(await_macro, async_await, futures_api, vec_remove_item)]

#[macro_use]
extern crate tokio;

use tokio::net::{self, TcpStream};
use tokio::prelude::*;

use tk_listen::ListenExt;

use miniserde::{json, Deserialize, Serialize};

use futures::sync::mpsc;

use std::collections::HashMap;
use std::env;
use std::io;
use std::mem::size_of;
use std::net::{SocketAddr, SocketAddrV6};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Serialize, Deserialize)]
struct RequestCommandPing {
    #[serde(rename = "p")]
    payload: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct ResponseCommandPing {
    #[serde(rename = "p")]
    payload: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct RequestCommandAnnounce {
    #[serde(rename = "l")]
    listen_port: u16,

    #[serde(rename = "p")]
    publickey: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct ResponseCommandAnnounce {
    #[serde(rename = "l")]
    listen_port: u16,

    #[serde(rename = "p")]
    publickey: Vec<u8>,

    #[serde(rename = "c")]
    connections: Vec<String>,
}

struct Shared {
    pub connections: Vec<SocketAddr>,
    pub passive_node_log: Vec<SocketAddr>,
    pub publickey: Vec<u8>,
    pub listen_port: u16,
    pub pending_requests: HashMap<(SocketAddr, u8), mpsc::UnboundedSender<String>>,
    pub peer_publickeys: HashMap<SocketAddr, Vec<u8>>,
}

impl Shared {
    fn new() -> Self {
        Self {
            connections: Vec::new(),
            passive_node_log: Vec::new(),
            publickey: Vec::new(),
            listen_port: 0,
            pending_requests: HashMap::new(),
            peer_publickeys: HashMap::new(),
        }
    }
}

async fn handle_conn(stream: &mut TcpStream, shared: Arc<Mutex<Shared>>) -> io::Result<()> {
    loop {
        let kind = stream.bytes().next().unwrap()?;
        let way = stream.bytes().next().unwrap()? > 0;

        let mut be_len = [0u8; size_of::<u32>()];
        await!(stream.read_exact_async(&mut be_len))?;

        let len = u32::from_be_bytes(be_len);

        let mut raw = vec![0u8; len as usize];
        await!(stream.read_exact_async(&mut raw))?;

        let body =
            String::from_utf8(raw).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;

        match kind {
            0 /* PING */ => {
                if !way /* REQUEST */ {
                    let request: RequestCommandPing = json::from_str(&body)
                        .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;

                    let response = json::to_string(&ResponseCommandPing {
                        payload: request.payload
                    });

                    let raw = response.as_bytes();
                    let be_len = (raw.len() as u32).to_be_bytes();

                    let way: u8 = if !way { 1 } else { 0 };

                    await!(stream.write_all_async(std::slice::from_ref(&kind)))?;
                    await!(stream.write_all_async(std::slice::from_ref(&way)))?;
                    await!(stream.write_all_async(&be_len))?;
                    await!(stream.write_all_async(&raw))?;
                } else if let Some(sender) = shared.lock().unwrap().pending_requests.remove(&(stream.peer_addr().unwrap(), kind)) {

                    let mut sender = sender.clone();
                    tokio::spawn_async(async move {
                        await!(sender.send_async(body)).is_err();
                    });
                }
            }
            1 /* ANNOUNCE */ => {
                if !way /* REQUEST */ {
                    let request: RequestCommandAnnounce = json::from_str(&body)
                        .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;

                    
                } else {
                    
                }
            }
            _ /* UNKNOWN */ => {
                
            }
        };
    }
}

fn main() {
    let bootstrap_peer: Option<String> = env::args().nth(1);

    let shared = Arc::new(Mutex::new(Shared::new()));

    if let Some(bootstrap_peer) = bootstrap_peer {
        if let Ok(bootstrap_peer) = bootstrap_peer.parse() {
            shared.lock().unwrap().passive_node_log.push(bootstrap_peer);
        }
    }

    let listener = net::TcpListener::bind(&SocketAddr::from(
        "[::1]:0".parse::<SocketAddrV6>().unwrap(),
    ))
    .unwrap();

    println!("listening on {}", listener.local_addr().unwrap());

    tokio::run_async(async move {
        let mut incoming = listener
            .incoming()
            .sleep_on_error(Duration::from_millis(100));

        while let Some(stream) = await!(incoming.next()) {
            if let Ok(mut stream) = stream {
                let peer_addr = stream.peer_addr().unwrap();

                shared.lock().unwrap().connections.push(peer_addr);

                let shared = shared.clone();
                tokio::spawn_async(async move {
                    if await!(handle_conn(&mut stream, shared.clone())).is_err() {
                        shared.lock().unwrap().connections.remove_item(&peer_addr);
                    }
                });
            }
        }
    });
}
