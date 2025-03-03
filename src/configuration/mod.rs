use crate::pool_mint::mining_pool::{CoinbaseOutput, PoolConfiguration};
use crate::proxy_wallet::proxy_config::{
    DownstreamDifficultyConfig, ProxyConfig, UpstreamDifficultyConfig,
};
use clap::Parser;
use core::panic;
use ext_config::{Config, File, FileFormat};
use key_utils::Secp256k1PublicKey;
use std::io::{self, Write};
use std::str::FromStr;
use stratum_common::bitcoin::secp256k1::Secp256k1;
use stratum_common::bitcoin::util::bip32::{self, DerivationPath, ExtendedPubKey};
use stratum_common::bitcoin::Network;
use tracing::{error, info, warn};

#[derive(Parser, Debug)]
#[clap(author = "Gary Krause", version, about)]
/// Application configuration
pub struct Args {
    /// whether to be verbose
    #[arg(short = 'v')]
    pub verbose: bool,

    /// Path to the proxy wallet configuration file
    #[arg(
        short = 'p',
        long = "proxy-config",
        default_value = "proxy-config.toml"
    )]
    pub proxy_config_path: String,

    /// Path to the pool mint configuration file
    #[arg(
        short = 'm',
        long = "pool-mint-config",
        default_value = "pool-mint-config.toml"
    )]
    pub pool_mint_config_path: String,

    /// The coinbase output address where mining rewards will be sent (SLIP-132 format)
    #[arg(short = 'c', long = "coinbase-output")]
    pub coinbase_output: Option<String>,

    /// The derivation path for the coinbase output (e.g. m/0/0)
    #[arg(short = 'd', long = "derivation-path", default_value = "m/84/1/0")]
    pub derivation_path: String,

    /// The Bitcoin network to use (mainnet not allowed)
    #[arg(short = 'n', long = "network", default_value = "testnet")]
    pub network: Network,

    /// Whether bitcoind is performing initial sync (extends wait time indefinitely)
    #[arg(long = "initial-sync")]
    pub initial_sync: bool,
}

fn derive_child_public_key(
    xpub: &ExtendedPubKey,
    path: &str,
) -> Result<ExtendedPubKey, bip32::Error> {
    let secp = Secp256k1::new();
    let derivation_path = DerivationPath::from_str(path)?;
    let child_pub_key = xpub.derive_pub(&secp, &derivation_path)?;
    info!(
        "\nPublic key derived from your Master Public Key -> {:?}",
        child_pub_key.to_pub().inner.to_string()
    );
    Ok(child_pub_key)
}

fn validate_xpub(input: &str) -> Result<ExtendedPubKey, String> {
    slip132::FromSlip132::from_slip132_str(input)
        .map_err(|x| format!("Invalid SLIP-132 extended public key: {:?}", x))
}

fn prompt_for_coinbase_output() -> io::Result<String> {
    let coinbase_output: ExtendedPubKey;
    loop {
        info!("Please enter the SLIP-132 pubkey of the coinbase output: ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        match validate_xpub(input) {
            Ok(x) => {
                coinbase_output = x;
                break;
            }
            Err(e) => {
                error!("Error: {}. Please try again.", e);
                continue;
            }
        }
    }
    info!("Valid SLIP-132 pubkey provided.");
    loop {
        info!("Please provide a derivation path. A hardened path will not work.");
        info!("Press enter to use the default: m/84/1/0");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if input.trim().is_empty() {
            input = "m/84/1/0".to_owned();
        }
        match derive_child_public_key(&coinbase_output, &input) {
            Ok(child_key) => {
                info!("Derived public key: {}", child_key.to_string());
                return Ok(child_key.to_pub().inner.to_string());
            }
            Err(e) => {
                error!("Failed to derive child key: {}", e);
                warn!("Be sure to provide a non-hardened key derivation");
                continue;
            }
        }
    }
}

pub fn create_default_pool_config() -> PoolConfiguration {
    PoolConfiguration {
        listen_address: "0.0.0.0:34254".to_string(),
        tp_address: "127.0.0.1:8442".to_string(),
        tp_authority_public_key: Some(
            Secp256k1PublicKey::from_str("9auqWEzQDVyd2oe1JVGFLMLHZtCo2FFqZwtKA5gd9xbuEu7PH72")
                .unwrap(),
        ),
        authority_public_key: Secp256k1PublicKey::from_str(
            "9auqWEzQDVyd2oe1JVGFLMLHZtCo2FFqZwtKA5gd9xbuEu7PH72",
        )
        .unwrap(),
        authority_secret_key: "mkDLTBBRxdBv998612qipDYoTK3YUrqLe8uWw7gu3iXbSrn2n"
            .parse()
            .unwrap(),
        cert_validity_sec: 3600,
        coinbase_outputs: vec![CoinbaseOutput::new(
            "P2WPKH".to_string(),
            "032a384861cb109a7b69b550601e4935ee30903be6b281f058a3c65c657938f8f8".to_string(),
        )],
        pool_signature: "potato".to_string(),
        #[cfg(feature = "test_only_allow_unencrypted")]
        test_only_listen_address_plain: "0.0.0.0:34250".to_string(),
    }
}

pub fn create_default_proxy_config(pool_config: &PoolConfiguration) -> ProxyConfig {
    // Parse the pool's listen address

    ProxyConfig {
        upstream_address: "127.0.0.1".to_string(),
        upstream_port: 34254,
        upstream_authority_pubkey: pool_config.authority_public_key,
        downstream_address: "0.0.0.0".to_string(),
        downstream_port: 34255,
        max_supported_version: 2,
        min_supported_version: 2,
        min_extranonce2_size: 8,
        downstream_difficulty_config: DownstreamDifficultyConfig {
            min_individual_miner_hashrate: 10_000_000_000_000.0,
            shares_per_minute: 6.0,
            submits_since_last_update: 0,
            timestamp_of_last_update: 0,
        },
        upstream_difficulty_config: UpstreamDifficultyConfig {
            channel_diff_update_interval: 60,
            channel_nominal_hashrate: 10_000_000_000_000.0,
            timestamp_of_last_update: 0,
            should_aggregate: false,
        },
    }
}

pub fn load_or_create_proxy_config(
    config_path: &str,
    pool_config: &PoolConfiguration,
) -> Result<ProxyConfig, Box<dyn std::error::Error>> {
    match Config::builder()
        .add_source(File::new(config_path, FileFormat::Toml))
        .build()
    {
        Ok(config) => {
            let mut proxy_config: ProxyConfig = config.try_deserialize()?;
            warn!(
                    "Overriding proxy upstream authority public key from config file with pool's authority key"
                );
            proxy_config.upstream_authority_pubkey = pool_config.authority_public_key;
            Ok(proxy_config)
        }
        Err(e) => {
            warn!("Failed to load proxy config ({}), using defaults", e);
            Ok(create_default_proxy_config(pool_config))
        }
    }
}

pub fn load_or_create_pool_config(
    config_path: &str,
) -> Result<PoolConfiguration, Box<dyn std::error::Error>> {
    match Config::builder()
        .add_source(File::new(config_path, FileFormat::Toml))
        .build()
    {
        Ok(config) => Ok(config.try_deserialize::<PoolConfiguration>()?),
        Err(e) => {
            warn!("Failed to load pool config ({}), using defaults", e);
            Ok(create_default_pool_config())
        }
    }
}

pub fn process_coinbase_output(
    coinbase_output: Option<String>,
    derivation_path: String,
) -> Result<String, Box<dyn std::error::Error>> {
    if coinbase_output.is_none() {
        return match prompt_for_coinbase_output() {
            Ok(x) => Ok(x),
            Err(e) => panic!("You borked it! {}", e),
        };
    }
    let coinbase_output = coinbase_output.unwrap(); // we already checked this!
    let coinbase_output = match validate_xpub(&coinbase_output) {
        Ok(xpub) => {
            // Derive child key
            match derive_child_public_key(&xpub, &derivation_path) {
                Ok(child_key) => {
                    info!(
                        "Used {} with derivation path {}",
                        coinbase_output, derivation_path
                    );
                    info!("Derived public key: {}", child_key.to_string());
                    child_key.to_pub().inner.to_string()
                }
                Err(e) => {
                    error!("Failed to derive child key: {}", e);
                    warn!("Be sure to provide an correctly formatted SLIP-132 and non-hardened key derivation");
                    prompt_for_coinbase_output()?
                }
            }
        }
        Err(e) => {
            error!("Invalid coinbase output provided: {}", e);
            prompt_for_coinbase_output()?
        }
    };
    Ok(coinbase_output)
}
