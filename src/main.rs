use log::{debug, info, error};
use tokio;
use tokio_util::sync::CancellationToken;
use clap::Parser;
use std::env;
use std::path::PathBuf;
use std::time::Duration;
use bitcoincore_rpc::{Auth, Client as BitcoinCoreClient, RpcApi};
use stratum_common::bitcoin;
use tokio::fs;
use anyhow::Result;

mod pool_mint;
mod proxy_wallet;
mod status;
mod error;
mod configuration;

use configuration::{Args, load_or_create_proxy_config, load_or_create_pool_config, process_coinbase_output};
use pool_mint::mining_pool::CoinbaseOutput;

const BITCOIN_CONF_TEMPLATE: &str = r#"
regtest=1
fallbackfee=0.0004
txindex=1
server=1
rpcuser=bitcoin
rpcpassword=bitcoin
zmqpubrawblock=tcp://127.0.0.1:{zmq_block_port}
zmqpubrawtx=tcp://127.0.0.1:{zmq_tx_port}
rpcworkqueue=1024
rpcthreads=64
deprecatedrpc=warnings

[regtest]
port={p2p_port}
bind=127.0.0.1:{p2p_port}
rpcport={rpc_port}
rpcbind=127.0.0.1:{rpc_port}
"#;

struct BitcoinNode {
    client: BitcoinCoreClient,
    data_dir: PathBuf,
}

impl BitcoinNode {
    async fn new(data_dir: PathBuf, network: bitcoin::Network) -> Result<Self> {
        // Create data directory if it doesn't exist
        fs::create_dir_all(&data_dir).await?;
        
        // Default ports
        let rpc_port = match network {
            bitcoin::Network::Regtest => 18443,
            bitcoin::Network::Testnet => 18332,
            bitcoin::Network::Signet => 38332,
            _ => return Err(anyhow::anyhow!("Unsupported network"))
        };
        
        let p2p_port = rpc_port + 1;
        let zmq_block_port = rpc_port + 2;
        let zmq_tx_port = rpc_port + 3;

        // Create bitcoin.conf
        let conf = BITCOIN_CONF_TEMPLATE
            .replace("{rpc_port}", &rpc_port.to_string())
            .replace("{p2p_port}", &p2p_port.to_string())
            .replace("{zmq_block_port}", &zmq_block_port.to_string())
            .replace("{zmq_tx_port}", &zmq_tx_port.to_string());

        fs::write(data_dir.join("bitcoin.conf"), conf).await?;

        // Start bitcoind process
        let bitcoind_path = which::which("bitcoind")?;
        let mut cmd = tokio::process::Command::new(bitcoind_path);
        cmd.arg(format!("-datadir={}", data_dir.display()));
        
        // Spawn bitcoind
        let child = cmd.spawn()?;
        
        // Create RPC client
        let rpc_url = format!("http://127.0.0.1:{}", rpc_port);
        let auth = Auth::UserPass("bitcoin".to_string(), "bitcoin".to_string());
        let client = BitcoinCoreClient::new(&rpc_url, auth)?;

        Ok(Self {
            client,
            data_dir,
        })
    }

    async fn wait_for_ready(&self, initial_sync: bool) -> Result<()> {
        use tokio::time::sleep;
        
        // Constants for normal startup
        const MAX_WAIT: Duration = Duration::from_secs(8 * 60); // 8 minutes
        const INITIAL_WAIT: Duration = Duration::from_secs(1);
        const MAX_RETRY_INTERVAL: Duration = Duration::from_secs(30);
        
        // Constants for initial sync
        const SYNC_CHECK_INTERVAL: Duration = Duration::from_secs(3600); // Check once per hour
        
        let start = std::time::Instant::now();
        let mut wait_time = INITIAL_WAIT;
        
        loop {
            match self.client.get_blockchain_info() {
                Ok(info) => {
                    let elapsed = start.elapsed();
                    if initial_sync {
                        if info.initial_block_download {
                            let progress = info.verification_progress * 100.0;
                            info!("Bitcoin Core syncing... {:.2}% complete", progress);
                            sleep(SYNC_CHECK_INTERVAL).await;
                            continue;
                        }
                    }
                    debug!("Bitcoin Core ready after {:?}", elapsed);
                    return Ok(());
                }
                Err(e) => {
                    debug!("Waiting for Bitcoin Core: {}", e);
                    
                    if !initial_sync && start.elapsed() >= MAX_WAIT {
                        return Err(anyhow::anyhow!("Timeout waiting for bitcoind after {:?}", MAX_WAIT));
                    }
                    
                    sleep(if initial_sync { SYNC_CHECK_INTERVAL } else { wait_time }).await;
                    
                    // Only increase wait time for normal startup
                    if !initial_sync {
                        wait_time = std::cmp::min(wait_time * 2, MAX_RETRY_INTERVAL);
                    }
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = Args::parse();
    
    // Ensure mainnet is not allowed
    if args.network == bitcoin::Network::Bitcoin {
        error!("Mainnet is not supported");
        return Err("Mainnet is not supported".into());
    }
    
    // Set the log level based on the verbose flag
    if args.verbose {
        env::set_var("RUST_LOG", "debug");
    } else {
        env::set_var("RUST_LOG", "info");
    }
    pretty_env_logger::init();
    
    debug!("DEBUG {args:?}");

    // Initialize Bitcoin Core
    info!("Starting Bitcoin Core{}...", 
        if args.initial_sync { " (initial sync mode)" } else { "" });
    let bitcoin_data_dir = PathBuf::from("bitcoin_data");
    let bitcoin_node = BitcoinNode::new(bitcoin_data_dir, args.network).await?;
    
    // Wait for Bitcoin Core to be ready
    info!("Waiting for Bitcoin Core to be ready...");
    bitcoin_node.wait_for_ready(args.initial_sync).await?;
    info!("Bitcoin Core is ready");

    let cancel_token = CancellationToken::new();
    let cancel_token_proxy = cancel_token.clone();
    let cancel_token_pool = cancel_token.clone();

    // Load or create default proxy config
    let proxy_settings = load_or_create_proxy_config(&args.proxy_config_path)?;
    info!("ProxyWallet Config: {:?}", &proxy_settings);

    // Load or create default pool config
    let mut pool_settings = load_or_create_pool_config(&args.pool_mint_config_path)?;
    info!("PoolMint Config: {:?}", &pool_settings);

    // Process coinbase output
    let coinbase_output = process_coinbase_output(&mut args)?;

    info!("Using coinbase output address: {}", coinbase_output);
    info!("Using derivation path: {}", args.derivation_path);
    info!("Using proxy config path: {}", args.proxy_config_path);
    info!("Using pool mint config path: {}", args.pool_mint_config_path);

    // Update pool settings with the validated coinbase output
    let coinbase_output = CoinbaseOutput::new(
        "P2WPKH".to_string(),  // Using P2WPKH for SLIP-132 xpub
        coinbase_output,
    );
    pool_settings.coinbase_outputs = vec![coinbase_output];

    // Run both services concurrently and handle Ctrl+C
    tokio::select! {
        proxy_result = proxy_wallet::run(proxy_settings, cancel_token_proxy) => {
            if let Err(e) = proxy_result {
                error!("ProxyWallet error: {}", e);
                cancel_token.cancel();
                return Err(e);
            }
        }
        pool_result = pool_mint::run(pool_settings, cancel_token_pool) => {
            if let Err(e) = pool_result {
                error!("PoolMint error: {}", e);
                cancel_token.cancel();
                return Err(e);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received Ctrl+C, initiating graceful shutdown...");
            cancel_token.cancel();
        }
    }

    info!("Shutdown complete");
    Ok(())
}
