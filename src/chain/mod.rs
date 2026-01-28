use std::process::{Child, Command, Stdio};

use color_eyre::{
    eyre::{eyre, Result},
    Section,
};
use reqwest::Client;
use tokio::time::{sleep, Duration};
use tracing::{debug, info, instrument};

use crate::build;
use crate::run_tests::cleanup::{clean_process_by_pid, cleanup_on_signal};
use crate::run_tests::types::BroadcastRecvBool;
use crate::setup::{check_foundry_deps, get_deps};
use crate::KIT_CACHE;

// important contract addresses:
//  https://gist.github.com/nick1udwig/273292fdfe94dd1c563f302df8bdfb74

// first account on anvil
const OWNER_ADDRESS: &str = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";

const ERC6551_REGISTRY: &str = "0x000000006551c19487814612e58FE06813775758";
const MULTICALL3: &str = "0xcA11bde05977b3631167028862bE2a173976CA11";
const CREATE2: &str = "0x4e59b44847b379578588920cA78FbF26c0B4956C";

const HYPERMAP_PROXY: &str = "0x000000000044C6B8Cb4d8f0F889a3E47664EAeda";
const HYPERMAP: &str = "0x000000000013a0486EBDc2DB1D7B4d1f7fCA92eD";
const HYPER_ACCOUNT: &str = "0x0000000000EDAd72076CBe7b9Cfa3751D5a85C97";
//const HYPER_ACCOUNT_MINTER: &str = "0xE01dCbD3Ed5f709874A1eA7a25677de18C8661c9";

const DOT_OS_TBA: &str = "0x9b3853358ede717fc7D4806cF75d7A4d4517A9C9";
const ZEROTH_TBA: &str = "0x809A598d9883f2Fb6B77382eBfC9473Fd6A857c9";

const HYPERMAP_PROXY_LONG: &str =
    "0x000000000000000000000000000000000044C6B8Cb4d8f0F889a3E47664EAeda";
const HYPERMAP_LONG: &str = "0x000000000000000000000000000000000013a0486EBDc2DB1D7B4d1f7fCA92eD";

const DEFAULT_MAX_ATTEMPTS: u16 = 16;

const PREDEPLOY_CONTRACTS: &[(&str, &str)] = &[
    (
        ERC6551_REGISTRY,
        include_str!("./bytecode/erc6551registry.txt"),
    ),
    (MULTICALL3, include_str!("./bytecode/multicall.txt")),
    (HYPER_ACCOUNT, include_str!("./bytecode/hyperaccount.txt")),
    (HYPERMAP_PROXY, include_str!("./bytecode/erc1967proxy.txt")),
    (HYPERMAP, include_str!("./bytecode/hypermap.txt")),
];

const STORAGE_SLOTS: &[(&str, &str, &str)] = &[
    // Implementation slot
    (
        HYPERMAP_PROXY,
        "0x360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc",
        HYPERMAP_LONG,
    ),
    // Hypermap immutable (set to proxy's own address)
    (
        HYPERMAP_PROXY,
        "0x0000000000000000000000000000000000000000000000000000000000000000",
        HYPERMAP_PROXY_LONG,
    ),
];

const TRANSACTIONS: &[(&str, &str)] = &[
    // initialize Hypermap: give ownership to OWNER_ADDRESS
    // cast calldata "initialize(address)" 0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266
    (
        HYPERMAP_PROXY,
        "0xc4d66de8000000000000000000000000f39fd6e51aad88f6f4ce6ab8827279cfffb92266",
    ),
    // CREATE2 deploy HyperAccountMinter (deployed at 0xE01dCbD3Ed5f709874A1eA7a25677de18C8661c9)
    (
        CREATE2,
        include_str!("./bytecode/deploy-hyperaccount-minter.txt"),
    ),
    // CREATE2 deploy HyperAccountPermissionedMinter
    (
        CREATE2,
        include_str!("./bytecode/deploy-hyperaccount-permissioned-minter.txt"),
    ),
    // CREATE2 deploy HyperAccount9CharCommitMinter
    (
        CREATE2,
        include_str!("./bytecode/deploy-hyperaccount-9char-commit-minter.txt"),
    ),
    (
        CREATE2,
        include_str!("./bytecode/deploy-hyperaccount-permissioned-minter-2.txt"),
    ),
    // mint .os
    //  NOTE: the account implementation here is not
    //        HyperAccount9CharCommitMinter like on mainnet.
    //        instead, we use HyperAccountMinter so that we
    //        can mint these nodes very easily when a new fake
    //        node is spun up
    // cast calldata "execute(address,uint256,bytes,uint8)" 0x000000000044C6B8Cb4d8f0F889a3E47664EAeda 0 $(cast calldata "mint(address,bytes,bytes,address)" 0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266 $(cast --from-ascii "os") $(cast calldata "initialize()") 0xE01dCbD3Ed5f709874A1eA7a25677de18C8661c9) 0
    (ZEROTH_TBA, include_str!("./bytecode/mint-os.txt")),
];

#[instrument(level = "trace", skip_all)]
async fn get_nonce(port: u16, client: &Client, address: &str) -> Result<u64> {
    let url = format!("http://localhost:{}", port);
    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionCount",
        "params": [address, "latest"],
        "id": 1
    });
    let response: serde_json::Value = client
        .post(&url)
        .json(&request_body)
        .send()
        .await?
        .json()
        .await?;

    let nonce_hex = response["result"]
        .as_str()
        .ok_or_else(|| eyre!("Invalid nonce response"))?
        .trim_start_matches("0x");

    let nonce = u64::from_str_radix(nonce_hex, 16)?;
    Ok(nonce)
}

#[instrument(level = "trace", skip_all)]
async fn execute_transaction(
    port: u16,
    client: &Client,
    from: &str,
    to: &str,
    data: &str,
    nonce: u64,
) -> Result<String> {
    let url = format!("http://localhost:{}", port);
    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_sendTransaction",
        "params": [{
            "from": from,
            "to": to,
            "data": data,
            "nonce": format!("0x{:x}", nonce),
            "gas": "0x500000",
        }],
        "id": 1
    });

    let res: serde_json::Value = client
        .post(&url)
        .json(&request_body)
        .send()
        .await?
        .json()
        .await?;

    if let Some(result) = res.get("result") {
        if let Some(result) = result.as_str() {
            let result = result.to_string();
            return Ok(result);
        }
        return Err(eyre!("unexpected result: {res}"));
    }
    if let Some(error) = res.get("error") {
        return Err(eyre!("{error}"));
    }
    return Err(eyre!("unexpected response: {res}"));
}

#[instrument(level = "trace", skip_all)]
async fn initialize_contracts(port: u16) -> Result<()> {
    let client = Client::new();
    let url = format!("http://localhost:{}", port);

    // impersonate owner account
    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "anvil_impersonateAccount",
        "params": [OWNER_ADDRESS],
        "id": 1
    });
    let _: serde_json::Value = client
        .post(&url)
        .json(&request_body)
        .send()
        .await?
        .json()
        .await?;

    // set storage slots
    for (address, slot, value) in STORAGE_SLOTS {
        let request_body: serde_json::Value = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "anvil_setStorageAt",
            "params": [address, slot, value],
            "id": 1
        });
        let _: serde_json::Value = client
            .post(&url)
            .json(&request_body)
            .send()
            .await?
            .json()
            .await?;
    }

    let mut nonce = get_nonce(port, &client, OWNER_ADDRESS).await?;

    // execute all transactions
    for (to, data) in TRANSACTIONS {
        match execute_transaction(port, &client, OWNER_ADDRESS, to, data, nonce).await {
            Ok(result) => debug!("Transaction to {to}:  {result}"),
            Err(e) => info!("Transaction failed: {e:?}"),
        }
        nonce += 1;
    }

    // stop impersonating
    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "anvil_stopImpersonatingAccount",
        "params": [OWNER_ADDRESS],
        "id": 1
    });
    let _: serde_json::Value = client
        .post(&url)
        .json(&request_body)
        .send()
        .await?
        .json()
        .await?;

    Ok(())
}

#[instrument(level = "trace", skip_all)]
async fn check_dot_os_tba(port: u16) -> Result<bool> {
    let client = Client::new();
    let url = format!("http://localhost:{}", port);

    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getCode",
        "params": [DOT_OS_TBA, "latest"],
        "id": 1
    });

    let response = client.post(&url).json(&request_body).send().await?;
    let result: serde_json::Value = response.json().await?;
    let code = result["result"].as_str().unwrap_or("0x");
    Ok(code != "0x")
}

#[instrument(level = "trace", skip_all)]
pub async fn start_chain(
    port: u16,
    mut recv_kill: BroadcastRecvBool,
    verbose: bool,
    tracing: bool,
) -> Result<Option<Child>> {
    let deps = check_foundry_deps()?;
    get_deps(
        deps,
        &mut recv_kill,
        false,
        verbose,
        build::DEFAULT_RUST_TOOLCHAIN,
    )
    .await?;

    info!("Checking for Anvil on port {}...", port);
    if wait_for_anvil(port, 1, None).await.is_ok() {
        if !check_dot_os_tba(port).await? {
            predeploy_contracts(port).await?;
            initialize_contracts(port).await?;
        }
        return Ok(None);
    }

    let mut args = vec!["--port".to_string(), port.to_string()];
    if tracing {
        args.push("--tracing".to_string());
    }
    let mut child = Command::new("anvil")
        .args(args)
        .current_dir(KIT_CACHE)
        .stdout(if verbose {
            Stdio::inherit()
        } else {
            Stdio::piped()
        })
        .spawn()?;

    info!("Waiting for Anvil to be ready on port {}...", port);
    if let Err(e) = wait_for_anvil(port, DEFAULT_MAX_ATTEMPTS, Some(recv_kill)).await {
        let _ = child.kill();
        return Err(e);
    }

    if !check_dot_os_tba(port).await? {
        if let Err(e) = predeploy_contracts(port).await {
            let _ = child.kill();
            return Err(e.wrap_err("Failed to pre-deploy contracts"));
        }

        if let Err(e) = initialize_contracts(port).await {
            let _ = child.kill();
            return Err(e.wrap_err("Failed to initialize contracts"));
        }
    }

    Ok(Some(child))
}

#[instrument(level = "trace", skip_all)]
async fn wait_for_anvil(
    port: u16,
    max_attempts: u16,
    mut recv_kill: Option<BroadcastRecvBool>,
) -> Result<()> {
    let client = Client::new();
    let url = format!("http://localhost:{}", port);

    for _ in 0..max_attempts {
        let request_body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "params": [],
            "id": 1
        });

        let response = client.post(&url).json(&request_body).send().await;

        match response {
            Ok(resp) if resp.status().is_success() => {
                let result: serde_json::Value = resp.json().await?;
                if let Some(block_number) = result["result"].as_str() {
                    if block_number.starts_with("0x") {
                        info!("Anvil is ready on port {}.", port);
                        return Ok(());
                    }
                }
            }
            _ => (),
        }

        if let Some(ref mut recv_kill) = recv_kill {
            tokio::select! {
                _ = sleep(Duration::from_millis(250)) => {}
                _ = recv_kill.recv() => {
                    return Err(eyre!("Received kill: bringing down anvil."));
                }
            }
        } else {
            sleep(Duration::from_millis(250)).await;
        }
    }

    Err(eyre!(
        "Failed to connect to Anvil on port {} after {} attempts",
        port,
        max_attempts
    )
    .with_suggestion(|| "Is port already occupied?"))
}

#[instrument(level = "trace", skip_all)]
async fn predeploy_contracts(port: u16) -> Result<()> {
    let client = Client::new();
    let url = format!("http://localhost:{}", port);

    for (address, bytecode) in PREDEPLOY_CONTRACTS {
        let request_body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getCode",
            "params": [address, "latest"],
            "id": 1
        });

        let response = client.post(&url).json(&request_body).send().await?;
        let result: serde_json::Value = response.json().await?;
        let code = result["result"].as_str().unwrap_or("0x");

        if code == "0x" {
            let request_body = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "anvil_setCode",
                "params": [address, bytecode.trim()],
                "id": 1
            });
            let _: serde_json::Value = client
                .post(&url)
                .json(&request_body)
                .send()
                .await?
                .json()
                .await?;
            info!("Deployed contract at {address}.");
        }
    }

    Ok(())
}

/// kit chain, alias to anvil
#[instrument(level = "trace", skip_all)]
pub async fn execute(port: u16, verbose: bool, tracing: bool) -> Result<()> {
    let (send_to_cleanup, mut recv_in_cleanup) = tokio::sync::mpsc::unbounded_channel();
    let (send_to_kill, _recv_kill) = tokio::sync::broadcast::channel(1);
    let recv_kill_in_cos = send_to_kill.subscribe();

    let handle_signals = tokio::spawn(cleanup_on_signal(send_to_cleanup.clone(), recv_kill_in_cos));

    let recv_kill_in_start_chain = send_to_kill.subscribe();
    let child = start_chain(port, recv_kill_in_start_chain, verbose, tracing).await?;
    let Some(mut child) = child else {
        return Err(eyre!(
            "Port {} is already in use by another anvil process",
            port
        ));
    };
    let child_id = child.id() as i32;

    let cleanup_anvil = tokio::spawn(async move {
        recv_in_cleanup.recv().await;
        clean_process_by_pid(child_id);
    });

    let _ = child.wait();

    let _ = handle_signals.await;
    let _ = cleanup_anvil.await;

    let _ = send_to_kill.send(true);

    Ok(())
}
