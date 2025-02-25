use anyhow::Result;
use clap::Parser;
use proxy_wallet::TranslatorSv2;
use tracing::{debug, error, info};
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
use pool_mint::{mining_pool::CoinbaseOutput, PoolSv2};

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

    // Initialize tracing subscriber
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .with_target(false)
        .init();

    debug!("DEBUG {args:?}");

    // // Initialize Bitcoin Core
    // info!(
    //     "Starting Bitcoin Core{}...",
    //     if args.initial_sync {
    //         " (initial sync mode)"
    //     } else {
    //         ""
    //     }
    // );
    // let bitcoin_data_dir = PathBuf::from("bitcoin_data");
    // let bitcoin_node = BitcoinNode::new(bitcoin_data_dir, args.network).await?;

    // // Wait for Bitcoin Core to be ready
    // info!("Waiting for Bitcoin Core to be ready...");
    // bitcoin_node.wait_for_ready(args.initial_sync).await?;
    // info!("Bitcoin Core is ready");

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

    let pool_task = tokio::spawn(async move {
        let pool = PoolSv2::new(pool_settings, cancel_token_pool);
        if let Err(e) = pool.start().await {
            error!("Pool task error: {}", e);
            return Err(e);
        }
        Ok(())
    });

    let proxy_task: tokio::task::JoinHandle<std::result::Result<(), ()>> = tokio::spawn(async move {
        let proxy = TranslatorSv2::new(proxy_settings, cancel_token_proxy);
        proxy.start().await;
        Ok(())
    });

    // Wait for both tasks to complete
    let (pool_result, proxy_result) = tokio::join!(pool_task, proxy_task);

    if let Err(e) = pool_result {
        error!("Pool task error: {}", e);
    }
    if let Err(e) = proxy_result {
        error!("Proxy task error: {}", e);
    }

    info!("Shutdown complete");

    Ok(())
}
