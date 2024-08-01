use color_eyre::eyre::{self, Result};
use reqwest::Client;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::str::FromStr;

use alloy::{
    network::{eip2718::Encodable2718, EthereumWallet, TransactionBuilder},
    primitives::{keccak256, Address, Bytes, U256},
    providers::{Provider, ProviderBuilder, RootProvider},
    pubsub::PubSubFrontend,
    rpc::client::WsConnect,
    rpc::types::eth::{TransactionInput, TransactionRequest},
    signers::local::PrivateKeySigner,
};
use alloy_sol_macro::sol;
use alloy_sol_types::SolCall;

sol! {
        function mint (
            address who,
            bytes calldata label,
            bytes calldata initialization,
            bytes calldata erc721Data,
            address implementation
        ) external returns (
            address tba
        );

        function get (
            bytes32 node
        ) external view returns (
            address tba,
            address owner,
            bytes data,
        );

        function note (
            bytes calldata note,
            bytes calldata data
        ) external returns (
            bytes32 notenode
        );

        // tba account
        function execute(
            address to,
            uint256 value,
            bytes calldata data,
            uint8 operation
        ) external payable returns (bytes memory returnData);


        struct Call {
            address target;
            bytes callData;
        }

        function aggregate(
            Call[] calldata calls
        ) external payable returns (uint256 blockNumber, bytes[] memory returnData);
}

const KIMAP_ADDRESS: &str = "0x0165878A594ca255338adfa4d48449f69242Eb8F";
const MULTICALL_ADDRESS: &str = "0xcA11bde05977b3631167028862bE2a173976CA11";
const KINO_ACCOUNT_IMPL: &str = "0xCf7Ed3AccA5a467e9e704C703E8D87F634fB0Fc9";
const FAKE_DOTDEV_TBA: &str = "0x69C30C0Cf0e9726f9eEF50bb74FA32711fA0B02D";

pub async fn execute(
    private_key: Option<String>,
    app_name: Option<String>,
    fakechain_port: u16,
    manifest_path: &str,
) -> Result<()> {
    let private_key = get_private_key(private_key)?;
    let app_name = get_app_name(app_name, manifest_path)?;

    let privkey_signer =
        PrivateKeySigner::from_str(&private_key).expect("Failed to create signer from private key");

    let wallet_address = privkey_signer.address();
    let wallet: EthereumWallet = privkey_signer.into();

    let endpoint = format!("ws://localhost:{}", fakechain_port);
    let ws = WsConnect::new(endpoint);
    let provider: RootProvider<PubSubFrontend> = ProviderBuilder::default().on_ws(ws).await?;

    let kimap = Address::from_str(KIMAP_ADDRESS)?;
    let multicall_address = Address::from_str(MULTICALL_ADDRESS)?;
    let fakedotdev_tba = Address::from_str(FAKE_DOTDEV_TBA)?;
    let kino_account_impl = Address::from_str(KINO_ACCOUNT_IMPL)?;

    // Create metadata calls
    let metadata_uri = get_metadata_uri(manifest_path)?;
    let metadata_hash = calculate_metadata_hash(&metadata_uri).await?;

    let metadata_uri_call = noteCall {
        note: "~metadata-uri".into(),
        data: metadata_uri.into(),
    }
    .abi_encode();

    let metadata_hash_call = noteCall {
        note: "~metadata-hash".into(),
        data: metadata_hash.into(),
    }
    .abi_encode();

    let calls = vec![
        Call {
            target: kimap,
            callData: metadata_uri_call.into(),
        },
        Call {
            target: kimap,
            callData: metadata_hash_call.into(),
        },
    ];

    let notes_multicall = aggregateCall { calls }.abi_encode();

    let init_call = executeCall {
        to: multicall_address,
        value: U256::from(0),
        data: notes_multicall.into(),
        operation: 1,
    }
    .abi_encode();

    let create_app_tba_call = mintCall {
        who: wallet_address,
        label: app_name.clone().into(),
        initialization: init_call.into(),
        erc721Data: Bytes::default(),
        implementation: kino_account_impl,
    }
    .abi_encode();

    let create_app_call = executeCall {
        to: kimap,
        value: U256::from(0),
        data: create_app_tba_call.into(),
        operation: 0,
    }
    .abi_encode();

    let nonce = provider.get_transaction_count(wallet_address).await?;

    let tx = TransactionRequest::default()
        .to(fakedotdev_tba)
        .input(TransactionInput::new(create_app_call.into()))
        .nonce(nonce)
        .with_chain_id(31337)
        .with_gas_limit(1_000_000)
        .with_max_priority_fee_per_gas(200_000_000_000)
        .with_max_fee_per_gas(300_000_000_000);

    let tx_envelope = tx.build(&wallet).await?;
    let tx_encoded = tx_envelope.encoded_2718();
    let tx_hash = provider.send_raw_transaction(&tx_encoded).await?;

    println!("App '{}' published successfully!", app_name);
    println!("Transaction hash: {:?}", tx_hash);
    Ok(())
}

fn get_metadata_uri(manifest_path: &str) -> Result<String> {
    let manifest_path = Path::new(manifest_path);
    if manifest_path.exists() {
        let contents = fs::read_to_string(manifest_path)?;
        let manifest: Value = serde_json::from_str(&contents)?;
        if let Some(uri) = manifest["metadata_uri"].as_str() {
            return Ok(uri.to_string());
        }
    }
    Err(eyre::eyre!("Metadata URI not found in the manifest file."))
}

async fn calculate_metadata_hash(metadata_uri: &str) -> Result<String> {
    let client = Client::new();
    let response = client.get(metadata_uri).send().await?;
    let metadata_text = response.text().await?;

    let _: Value = serde_json::from_str(&metadata_text)?;

    let hash = keccak256(metadata_text.as_bytes());

    Ok(format!("0x{}", hex::encode(hash)))
}

fn get_private_key(cli_key: Option<String>) -> Result<String> {
    if let Some(key) = cli_key {
        return Ok(key);
    }

    if let Ok(key) = std::env::var("PRIVATE_KEY") {
        return Ok(key);
    }

    let env_path = Path::new(".env");
    if env_path.exists() {
        let contents = fs::read_to_string(env_path)?;
        for line in contents.lines() {
            if line.starts_with("PRIVATE_KEY=") {
                return Ok(line.trim_start_matches("PRIVATE_KEY=").to_string());
            }
        }
    }

    Err(eyre::eyre!("Private key not found. Please provide it as an argument, set the PRIVATE_KEY environment variable, or include it in a .env file."))
}

fn get_app_name(cli_name: Option<String>, manifest_path: &str) -> Result<String> {
    if let Some(name) = cli_name {
        return Ok(name);
    }

    let manifest_path = Path::new(manifest_path);
    if manifest_path.exists() {
        let contents = fs::read_to_string(manifest_path)?;
        let manifest: Value = serde_json::from_str(&contents)?;
        if let Some(name) = manifest["name"].as_str() {
            return Ok(name.to_string());
        }
    }

    Err(eyre::eyre!(
        "App name not found. Please provide it as an argument or include it in the manifest file."
    ))
}
