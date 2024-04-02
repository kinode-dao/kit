use std::collections::HashMap;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task;
use tokio_tungstenite::{accept_async, connect_async, tungstenite::protocol::Message::{Binary, Ping, Pong, Text}, WebSocketStream};
use tracing::{info, error, instrument};

use crate::run_tests::types::*;
use crate::run_tests::tester_types as tt;

type Sender = mpsc::Sender<tt::KernelMessage>;
type Receiver = mpsc::Receiver<tt::KernelMessage>;

struct Connection {
    send_to_node: Sender,
    send_to_kill_conn: mpsc::Sender<bool>,
}

type Connections = HashMap<String, Connection>;

pub const PING_TEXT: &str = "Hello, Kinode network router?";
pub const PONG_TEXT: &str = "Yes hello, Kinode network router department.";

async fn handshake(
    stream: TcpStream,
) -> anyhow::Result<Option<(String, WebSocketStream<TcpStream>)>> {
    let ws_stream = accept_async(stream).await?;
    let (mut send_to_ws, mut recv_from_ws) = ws_stream.split();

    let Some(Ok(message)) = recv_from_ws.next().await else {
        return Err(anyhow::anyhow!(
            "Handshake failed: first message was not received properly"
        ));
    };
    match message {
        Ping(message) => {
            let message = String::from_utf8(message)?;
            if message != PING_TEXT.to_string() {
                return Err(anyhow::anyhow!("Received Ping with unexpected message {}", message))
            }
            if let Err(_) = send_to_ws
                .send(Pong(PONG_TEXT.as_bytes().to_vec()))
                .await
            {
                return Err(anyhow::anyhow!("Failed to reply to Ping with Pong"))
            }
            Ok(None)
        },
        Text(identifier) => {
            let ws_stream = send_to_ws.reunite(recv_from_ws)?;
            Ok(Some((identifier, ws_stream)))
        },
        _ => Err(anyhow::anyhow!("Handshake failed: first message was not Ping or Text")),
    }
}

async fn handle_connection(
    ws_stream: WebSocketStream<TcpStream>,
    mut recv_in_node: Receiver,
    mut recv_kill_in_conn: mpsc::Receiver::<bool>,
    send_to_loop: Sender
) {
    let (mut send_to_ws, mut recv_from_ws) = ws_stream.split();

    loop {
        tokio::select! {
            Some(ref kernel_message) = recv_in_node.recv() => {
                if let Err(e) = send_to_ws
                    .send(Binary(rmp_serde::to_vec(kernel_message).unwrap()))
                    .await
                {
                    error!("Error sending message: {}", e);
                    break;
                }
            },
            Some(Ok(message)) = recv_from_ws.next() => {
                if let Binary(ref bin) = message {
                    let kernel_message = rmp_serde::from_slice(bin).unwrap();
                    if let Err(e) = send_to_loop.send(kernel_message).await {
                        error!("Error forwarding message: {}", e);
                        break;
                    }
                }
            },
            _ = recv_kill_in_conn.recv() => {
                break;
            },
        }
    }
}

#[instrument(level = "trace", err, skip_all)]
pub async fn execute(
    port: u16,
    _defects: NetworkRouterDefects,
    mut recv_kill_in_router: BroadcastRecvBool,
) -> anyhow::Result<()> {
    let (send_to_loop, mut recv_in_loop): (Sender, Receiver) = mpsc::channel(32);
    let mut connections: Connections = HashMap::new();

    let url = format!("127.0.0.1:{}", port);

    // Try hitting given port with Ping/Pong protocol to determine
    //  if another fake node already has a network_router running.
    if let Ok((ws_stream, _)) = connect_async(format!("ws://{}", url)).await {
        let (mut send_to_ws, mut recv_from_ws) = ws_stream.split();

        send_to_ws.send(Ping(PING_TEXT.as_bytes().to_vec())).await?;
        if let Some(Ok(Pong(message))) = recv_from_ws.next().await {
            if String::from_utf8(message).unwrap_or_default() == PONG_TEXT.to_string() {
                return Ok(());
            }
        };
    }

    // Didn't find one already running? Start network_router.
    let listener = TcpListener::bind(&url).await?;

    info!("network_router: online at {}\r", port);

    loop {
        tokio::select! {
            Ok((stream, _)) = listener.accept() => {
                let send_to_loop = send_to_loop.clone();
                match handshake(stream).await {
                    Ok(Some((identifier, ws_stream))) => {
                        let (send_to_node, recv_in_node) = mpsc::channel(32);
                        let (send_to_kill_conn, recv_kill_in_conn) = mpsc::channel::<bool>(1);
                        connections.insert(
                            identifier,
                            Connection { send_to_node, send_to_kill_conn },
                        );
                        task::spawn(handle_connection(
                            ws_stream, recv_in_node, recv_kill_in_conn, send_to_loop,
                        ));
                    },
                    Ok(None) => {},
                    Err(e) => error!("Handshake error: {}", e),
                }
            },
            Some(kernel_message) = recv_in_loop.recv() => {
                if let Some(Connection { send_to_node, .. }) = connections.get(&kernel_message.target.node) {
                    let _ = send_to_node.send(kernel_message).await;
                }
            },
            _ = recv_kill_in_router.recv() => {
                for connection in connections.values() {
                    let _ = connection.send_to_kill_conn.send(true).await;
                }
                break;
            },
        }
    }

    Ok(())
}
