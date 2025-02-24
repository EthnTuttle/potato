use anyhow::Result;
use bitcoincore_rpc::{Auth, Client as BitcoinCoreClient, RpcApi};
use log::{debug, info};
use std::path::PathBuf;
use std::time::Duration;
use stratum_common::bitcoin;
use tokio::fs;

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

pub struct BitcoinNode {
    client: BitcoinCoreClient,
    data_dir: PathBuf,
}

impl BitcoinNode {
    pub async fn new(data_dir: PathBuf, network: bitcoin::Network) -> Result<Self> {
        // ... existing code ...
        fs::create_dir_all(&data_dir).await?;
        
        let rpc_port = match network {
            bitcoin::Network::Regtest => 18443,
            bitcoin::Network::Testnet => 18332,
            bitcoin::Network::Signet => 38332,
            _ => return Err(anyhow::anyhow!("Unsupported network"))
        };
        
        let p2p_port = rpc_port + 1;
        let zmq_block_port = rpc_port + 2;
        let zmq_tx_port = rpc_port + 3;

        let conf = BITCOIN_CONF_TEMPLATE
            .replace("{rpc_port}", &rpc_port.to_string())
            .replace("{p2p_port}", &p2p_port.to_string())
            .replace("{zmq_block_port}", &zmq_block_port.to_string())
            .replace("{zmq_tx_port}", &zmq_tx_port.to_string());

        fs::write(data_dir.join("bitcoin.conf"), conf).await?;

        let bitcoind_path = which::which("bitcoind")?;
        let mut cmd = tokio::process::Command::new(bitcoind_path);
        cmd.arg(format!("-datadir={}", data_dir.display()));
        
        let child = cmd.spawn()?;
        
        let rpc_url = format!("http://127.0.0.1:{}", rpc_port);
        let auth = Auth::UserPass("bitcoin".to_string(), "bitcoin".to_string());
        let client = BitcoinCoreClient::new(&rpc_url, auth)?;

        Ok(Self {
            client,
            data_dir,
        })
    }

    pub async fn wait_for_ready(&self, initial_sync: bool) -> Result<()> {
        // ... existing wait_for_ready implementation ...
        use tokio::time::sleep;
        
        const MAX_WAIT: Duration = Duration::from_secs(8 * 60);
        const INITIAL_WAIT: Duration = Duration::from_secs(1);
        const MAX_RETRY_INTERVAL: Duration = Duration::from_secs(30);
        const SYNC_CHECK_INTERVAL: Duration = Duration::from_secs(3600);
        
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
                    
                    if !initial_sync {
                        wait_time = std::cmp::min(wait_time * 2, MAX_RETRY_INTERVAL);
                    }
                }
            }
        }
    }
} 