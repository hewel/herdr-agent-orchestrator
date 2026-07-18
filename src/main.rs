use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use herdr_harness_coordinator::{
    broker::{BrokerRequest, BrokerServer, call},
    core::Coordinator,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

#[derive(Debug, Parser)]
#[command(name = "herdr-harness-coordinator", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Own `SQLite` state and the local JSONL Unix socket.
    Daemon {
        /// Durable state directory.
        #[arg(long, env = "HERDR_PLUGIN_STATE_DIR")]
        state_dir: PathBuf,
        /// Unix socket path; defaults beneath the state directory.
        #[arg(long)]
        socket: Option<PathBuf>,
    },
    /// Send one `BrokerRequest` JSON value from stdin and print one response.
    Call {
        /// Broker Unix socket.
        #[arg(long, env = "HERDR_COORDINATOR_SOCKET")]
        socket: PathBuf,
    },
    /// Proxy newline-delimited `BrokerRequest` values between stdio and the broker.
    StdioProxy {
        /// Broker Unix socket.
        #[arg(long, env = "HERDR_COORDINATOR_SOCKET")]
        socket: PathBuf,
    },
    /// Run one pane-resident Worker Host and its persistent native Harness.
    WorkerHost {
        /// Opaque Session capability passed by the Herdr Worker pane launch.
        #[arg(long)]
        session_id: String,
        /// Durable plugin state directory.
        #[arg(long, env = "HERDR_PLUGIN_STATE_DIR")]
        state_dir: PathBuf,
        /// Broker Unix socket; defaults beneath the state directory.
        #[arg(long, env = "HERDR_COORDINATOR_SOCKET")]
        socket: Option<PathBuf>,
    },
    /// Render the durable Harness Network popup snapshot.
    Popup {
        /// Active Supervisor capability used for authorized popup queries.
        #[arg(long, env = "HERDR_SUPERVISOR_CAPABILITY")]
        supervisor_capability: String,
        /// Durable plugin state directory.
        #[arg(long, env = "HERDR_PLUGIN_STATE_DIR")]
        state_dir: PathBuf,
        /// Broker Unix socket; defaults beneath the state directory.
        #[arg(long, env = "HERDR_COORDINATOR_SOCKET")]
        socket: Option<PathBuf>,
    },
    /// Serve identity-bound Coordinator tools over MCP stdio.
    Mcp {
        /// Harness Session capability retained by this proxy process.
        #[arg(long, env = "HERDR_HARNESS_CAPABILITY")]
        session_capability: String,
        /// Durable plugin state directory.
        #[arg(long, env = "HERDR_PLUGIN_STATE_DIR")]
        state_dir: PathBuf,
        /// Broker Unix socket; defaults beneath the state directory.
        #[arg(long, env = "HERDR_COORDINATOR_SOCKET")]
        socket: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_target(false)
        .try_init()
        .ok();
    match Cli::parse().command {
        Command::Daemon { state_dir, socket } => run_daemon(state_dir, socket).await,
        Command::Call { socket } => run_call(socket).await,
        Command::StdioProxy { socket } => run_proxy(socket).await,
        Command::WorkerHost {
            session_id,
            state_dir,
            socket,
        } => {
            let socket = socket.unwrap_or_else(|| state_dir.join("coordinator.sock"));
            herdr_harness_coordinator::host::run_worker_host(&socket, &state_dir, session_id).await
        }
        Command::Popup {
            supervisor_capability,
            state_dir,
            socket,
        } => {
            let socket = socket.unwrap_or_else(|| state_dir.join("coordinator.sock"));
            herdr_harness_coordinator::host::run_popup(&socket, supervisor_capability).await
        }
        Command::Mcp {
            session_capability,
            state_dir,
            socket,
        } => {
            let socket = socket.unwrap_or_else(|| state_dir.join("coordinator.sock"));
            herdr_harness_coordinator::mcp::from_bearer(&socket, session_capability)?
                .run_stdio()
                .await
        }
    }
}

async fn run_daemon(state_dir: PathBuf, socket: Option<PathBuf>) -> Result<()> {
    let coordinator = Arc::new(Coordinator::open(&state_dir).await?);
    let socket = socket.unwrap_or_else(|| state_dir.join("coordinator.sock"));
    let server = BrokerServer::bind(coordinator, &socket).await?;
    let result = tokio::select! {
        result = server.serve() => result.map_err(anyhow::Error::from),
        signal = tokio::signal::ctrl_c() => signal.context("waiting for shutdown signal"),
    };
    if let Err(error) = tokio::fs::remove_file(&socket).await
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(%error, path = %socket.display(), "failed to remove owned broker socket");
    }
    result
}

async fn run_call(socket: PathBuf) -> Result<()> {
    let mut input = Vec::new();
    tokio::io::stdin()
        .read_to_end(&mut input)
        .await
        .context("reading BrokerRequest from stdin")?;
    if input.len() > herdr_harness_coordinator::broker::MAX_BROKER_FRAME_BYTES {
        bail!("request exceeds the 1 MiB broker frame limit");
    }
    let request: BrokerRequest =
        serde_json::from_slice(&input).context("decoding BrokerRequest JSON")?;
    let response = call(&socket, &request).await?;
    let mut output = serde_json::to_vec(&response).context("encoding BrokerResponse")?;
    output.push(b'\n');
    tokio::io::stdout()
        .write_all(&output)
        .await
        .context("writing BrokerResponse to stdout")
}

async fn run_proxy(socket: PathBuf) -> Result<()> {
    let mut input = BufReader::new(tokio::io::stdin());
    let mut output = tokio::io::stdout();
    loop {
        let mut frame = Vec::new();
        let read = input
            .read_until(b'\n', &mut frame)
            .await
            .context("reading proxy frame")?;
        if read == 0 {
            return Ok(());
        }
        if frame.len() > herdr_harness_coordinator::broker::MAX_BROKER_FRAME_BYTES {
            bail!("request exceeds the 1 MiB broker frame limit");
        }
        let request: BrokerRequest =
            serde_json::from_slice(&frame).context("decoding proxy BrokerRequest")?;
        let response = call(&socket, &request).await?;
        let mut encoded = serde_json::to_vec(&response).context("encoding BrokerResponse")?;
        encoded.push(b'\n');
        output
            .write_all(&encoded)
            .await
            .context("writing proxy response")?;
        output.flush().await.context("flushing proxy response")?;
    }
}
