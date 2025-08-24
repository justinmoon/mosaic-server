use mosaic_core::SecretKey;
use mosaic_net::{Approval, Approver};
use mosaic_server::{Logger, Server, ServerConfig};
use tokio::signal::unix::{SignalKind, signal};

use std::net::SocketAddr;

pub struct Denier;

impl Approver for Denier {
    fn is_client_allowed(&self, _: SocketAddr) -> Approval {
        Approval::Approve
    }
}

pub struct Log;

impl Logger for Log {
    fn log_client_error(&self, e: mosaic_server::Error) {

        // Swallow some boring errors that clutter the log files:
        match e.inner {
            mosaic_server::InnerError::MosaicNet(ref n) => match &n.inner {
                mosaic_net::InnerError::StatelessRetryRequired => return,
                mosaic_net::InnerError::ConnectionError(qce) => match qce {
                    quinn::ConnectionError::ConnectionClosed(_) => return,
                    quinn::ConnectionError::ApplicationClosed(_) => return,
                    _ => { },
                },
                _ => { },
            },
            _ => { },
        }

        eprintln!("{e}");
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let secret_key =
        SecretKey::from_printable("mosec06ayb687prmw8abtuum9bps5hjmfz5ffyft3b4jeznn3htppf3kto")?;
    println!("SERVER PUBLIC KEY IS {}", secret_key.public());

    let server_socket: SocketAddr = "127.0.0.1:8081".parse()?;
    println!("SERVER ENDPOINT IS {}", server_socket);

    let denier = Denier;
    let logger = Log;

    let server = Server::new(ServerConfig {
        secret_key,
        socket_addr: server_socket,
        approver: denier,
        logger,
    })?;

    let mut interrupt_signal = signal(SignalKind::interrupt())?;
    let mut quit_signal = signal(SignalKind::quit())?;

    loop {
        tokio::select! {
            v = interrupt_signal.recv() => if v.is_some() {
                eprintln!("SIGINT");
                server.trigger_shut_down(0);
            },
            v = quit_signal.recv() => if v.is_some() {
                eprintln!("SIGQUIT");
                server.trigger_shut_down(0);
            },
            r = server.run() => {
                if let Err(e) = r {
                    eprintln!("{e}");
                }
                break;
            }
        }
    }

    server.wait_for_shut_down().await;

    Ok(())
}
