use anyhow::Result;
use clap::Parser;
use log::{debug, error, info};
use std::env;
use std::path::PathBuf;
use stratum_common::bitcoin;
use tokio;
use tokio_util::sync::CancellationToken;

mod bitcoin_node;
mod configuration;
mod error;
mod pool_mint;
mod proxy_wallet;
mod status;

use bitcoin_node::BitcoinNode;
use configuration::{
    load_or_create_pool_config, load_or_create_proxy_config, process_coinbase_output, Args,
};
use pool_mint::mining_pool::CoinbaseOutput;

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
    info!(
        "Starting Bitcoin Core{}...",
        if args.initial_sync {
            " (initial sync mode)"
        } else {
            ""
        }
    );
    let bitcoin_data_dir = PathBuf::from("bitcoin_data");
    let bitcoin_node = BitcoinNode::new(bitcoin_data_dir, args.network).await?;

    // Wait for Bitcoin Core to be ready
    info!("Waiting for Bitcoin Core to be ready...");
    bitcoin_node.wait_for_ready(args.initial_sync).await?;
    info!("Bitcoin Core is ready");

    let cancel_token = CancellationToken::new();
    let cancel_token_proxy = cancel_token.clone();
    let cancel_token_pool = cancel_token.clone();
 
    // Load or create default pool config
    let mut pool_settings = load_or_create_pool_config(&args.pool_mint_config_path)?;
    info!("PoolMint Config: {:?}", &pool_settings);

    // Load or create default proxy config
    let proxy_settings = load_or_create_proxy_config(&args.proxy_config_path, &pool_settings)?;
    info!("ProxyWallet Config: {:?}", &proxy_settings);
    
    // Process coinbase output
    let coinbase_output = process_coinbase_output(&mut args)?;

    info!("Using coinbase output address: {}", coinbase_output);
    info!("Using derivation path: {}", args.derivation_path);
    info!("Using proxy config path: {}", args.proxy_config_path);
    info!(
        "Using pool mint config path: {}",
        args.pool_mint_config_path
    );

    // Update pool settings with the validated coinbase output
    let coinbase_output = CoinbaseOutput::new(
        "P2WPKH".to_string(), // Using P2WPKH for SLIP-132 xpub
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
