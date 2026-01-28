use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use color_eyre::{
    eyre::{eyre, Result},
    Section,
};
use reqwest::Client;
use serde::Deserialize;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, instrument};

use crate::build;
use crate::run_tests::cleanup::{clean_process_by_pid, cleanup_on_signal};
use crate::run_tests::types::BroadcastRecvBool;
use crate::setup::{check_foundry_deps, get_deps};
use crate::KIT_CACHE;

// First account on anvil
const OWNER_ADDRESS: &str = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";

const HYPERMAP: &str = "0x000000000044C6B8Cb4d8f0F889a3E47664EAeda";

//.os Token ID
const DOT_OS_TOKEN_ID: &str = "0xdeeac81ae11b64e7cab86d089c306e5d223552a630f02633ce170d2786ff1bbd";

const DOT_OS_LABAL_HEX: &str = "0x6f73"; // "os" label (2 bytes)

const TBA_OF_SELECTOR: &str = "0x27244d1e";

const DEFAULT_MAX_ATTEMPTS: u16 = 16;

const DEFAULT_CONFIG: &str = include_str!("./Contracts.toml");

// Verify Hypermap proxy - symbol()
const SYMBOL_CALLDATA: &str = "0x95d89b41";

// Verify ERC6551 Registry - account(address,uint256,address,uint256,uint256)
// Using fake data: implementation=0x0, chainId=1, tokenContract=0x0, tokenId=1, salt=0
const VERIFY_REGISTRY_CALLDATA: &str = "0x8a54c52f\
    0000000000000000000000000000000000000000000000000000000000000000\
    0000000000000000000000000000000000000000000000000000000000000001\
    0000000000000000000000000000000000000000000000000000000000000000\
    0000000000000000000000000000000000000000000000000000000000000001\
    0000000000000000000000000000000000000000000000000000000000000000";

// Verify Multicall3 - getBasefee()
const VERIFY_MULTICALL_CALLDATA: &str = "0x3e64a696";

#[derive(Debug, Clone)]
pub struct ContractAddresses {
    hypermap_proxy: String,
    hypermap_impl: String,
    hyperaccount: String,
    erc6551registry: String,
    multicall: String,
    create2: String,
    hyper_9char_commit_minter: Option<String>,
    hyper_permissioned_minter: Option<String>,
    zeroth_tba: Option<String>,
    dot_os_tba: Option<String>,
    other_contracts: HashMap<String, String>,
}

impl ContractAddresses {
    fn from_config(config: &ChainConfig, deployed: &HashMap<String, String>) -> Result<Self> {
        let resolve = |name: &str| -> Result<String> {
            if let Some(addr) = deployed.get(name) {
                return Ok(addr.clone());
            }
            if let Some(addr) = config.get_address_by_name(name) {
                return Ok(addr);
            }
            Err(eyre!("Missing '{}' in config and deployed contracts", name))
        };

        let resolve_optional = |name: &str| -> Option<String> {
            if let Some(addr) = deployed.get(name) {
                return Some(addr.clone());
            }
            config.get_address_by_name(name)
        };

        let mut other_contracts = HashMap::new();
        for contract in &config.contracts {
            if let Some(name) = &contract.name {
                if !matches!(
                    name.as_str(),
                    "hypermap-proxy"
                        | "hypermap-impl"
                        | "hyperaccount"
                        | "erc6551registry"
                        | "multicall"
                        | "create2"
                        | "hyperaccount-9char-commit-minter"
                        | "hyperaccount-permissioned-minter"
                ) {
                    if let Some(addr) = deployed.get(name) {
                        other_contracts.insert(name.clone(), addr.clone());
                    }
                }
            }
        }

        Ok(Self {
            hypermap_proxy: resolve("hypermap-proxy")?,
            hypermap_impl: resolve("hypermap-impl")?,
            hyperaccount: resolve("hyperaccount")?,
            erc6551registry: resolve("erc6551registry")?,
            multicall: resolve("multicall")?,
            create2: resolve("create2")?,
            hyper_9char_commit_minter: resolve_optional("hyperaccount-9char-commit-minter"),
            hyper_permissioned_minter: resolve_optional("hyperaccount-permissioned-minter"),
            zeroth_tba: None,
            dot_os_tba: None,
            other_contracts,
        })
    }
    fn print_summary(&self) {
        info!("Contract Addresses:");
        info!("{} hypermap_proxy", self.hypermap_proxy);
        info!("{} hypermap_impl", self.hypermap_impl);
        info!("{} hyperaccount", self.hyperaccount);
        info!("{} erc6551registry", self.erc6551registry);
        info!("{} multicall", self.multicall);
        info!("{} create2", self.create2);
        if let Some(addr) = &self.hyper_9char_commit_minter {
            info!("{} 9char_commit_minter", addr);
        }
        if let Some(addr) = &self.hyper_permissioned_minter {
            info!("{} permissioned_minter", addr);
        }

        if !self.other_contracts.is_empty() {
            for (name, addr) in &self.other_contracts {
                info!("{} {}", addr, name);
            }
        }

        if let Some(addr) = &self.zeroth_tba {
            info!("{} zeroth_tba (minted)", addr);
        }
        if let Some(addr) = &self.dot_os_tba {
            info!("{} dot_os_tba (minted)", addr);
        }
    }
}

#[derive(Debug, Deserialize)]
struct ChainConfig {
    #[serde(default)]
    contracts: Vec<ContractConfig>,

    #[serde(default)]
    transactions: Vec<TransactionConfig>,
}

#[derive(Debug, Deserialize)]
struct ContractConfig {
    #[serde(default)]
    name: Option<String>,

    #[serde(default)]
    contract_json_path: Option<String>,

    #[serde(default)]
    address: Option<String>,

    #[serde(default)]
    bytecode: Option<String>,

    #[serde(default)]
    deployed_bytecode_path: Option<String>,

    #[serde(default)]
    constructor_args: Vec<ConstructorArg>,

    #[serde(default)]
    storage: HashMap<String, StorageValue>,
}

#[derive(Debug, Deserialize, Clone)]
struct TransactionConfig {
    #[serde(default)]
    name: Option<String>,

    target: String,

    #[serde(default)]
    function_signature: Option<String>,

    #[serde(default)]
    args: Vec<ConstructorArg>,

    #[serde(default)]
    data: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct ConstructorArg {
    #[serde(rename = "type")]
    arg_type: String,
    value: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
enum StorageValue {
    String(String),
    Number(u64),
}

impl StorageValue {
    fn resolve(&self, deployed: &HashMap<String, String>) -> Result<String> {
        match self {
            StorageValue::String(s) => {
                if let Some(name) = s.strip_prefix('#') {
                    deployed
                        .get(name)
                        .cloned()
                        .ok_or_else(|| eyre!("Reference to unknown contract: #{}", name))
                } else {
                    Ok(s.clone())
                }
            }
            StorageValue::Number(n) => Ok(n.to_string()),
        }
    }

    fn to_hex_string(&self, deployed: &HashMap<String, String>) -> Result<String> {
        let resolved = self.resolve(deployed)?;

        if resolved.starts_with("0x") {
            Ok(format!("0x{:0>64}", resolved.trim_start_matches("0x")))
        } else if let Ok(num) = resolved.parse::<u64>() {
            Ok(format!("0x{:0>64x}", num))
        } else {
            Ok(format!("0x{:0>64}", resolved))
        }
    }
}

impl ConstructorArg {
    fn resolve_value(&self, deployed: &HashMap<String, String>) -> Result<String> {
        if let Some(name) = self.value.strip_prefix('#') {
            deployed
                .get(name)
                .cloned()
                .ok_or_else(|| eyre!("Reference to unknown contract in constructor: #{}", name))
        } else {
            Ok(self.value.clone())
        }
    }
}

impl ChainConfig {
    fn get_address_by_name(&self, name: &str) -> Option<String> {
        self.contracts
            .iter()
            .find(|c| c.name.as_deref() == Some(name))
            .and_then(|c| c.address.clone())
    }
}

struct ConfigSet<'a> {
    default: &'a ChainConfig,
    custom: Option<&'a ChainConfig>,
}

impl<'a> ConfigSet<'a> {
    fn new(default: &'a ChainConfig, custom: Option<&'a ChainConfig>) -> Self {
        Self { default, custom }
    }

    fn active(&self) -> &ChainConfig {
        self.custom.unwrap_or(self.default)
    }

    async fn process(&self, port: u16) -> Result<HashMap<String, String>> {
        process_configs(port, Some(self.default), self.custom).await
    }
}

fn normalize_slot(slot: &str) -> String {
    if slot.starts_with("0x") {
        format!("0x{:0>64}", slot.trim_start_matches("0x"))
    } else if let Ok(num) = slot.parse::<u64>() {
        format!("0x{:0>64x}", num)
    } else {
        format!("0x{:0>64}", slot)
    }
}

fn load_config(config_path: &PathBuf) -> Result<Option<ChainConfig>> {
    if !config_path.exists() {
        debug!("Config file not found at {:?}, skipping", config_path);
        return Ok(None);
    }

    let content =
        fs::read_to_string(config_path).map_err(|e| eyre!("Failed to read config file: {}", e))?;

    let config: ChainConfig =
        toml::from_str(&content).map_err(|e| eyre!("Failed to parse config file: {}", e))?;

    info!("Loaded config from {:?}", config_path);
    Ok(Some(config))
}

fn load_default_config() -> Result<ChainConfig> {
    toml::from_str(DEFAULT_CONFIG).map_err(|e| eyre!("Failed to parse default config: {}", e))
}

/// Load bytecode from JSON artifact
fn load_bytecode_from_json(path: &str, field: &str) -> Result<String> {
    let content = fs::read_to_string(path)
        .map_err(|e| eyre!("Failed to read bytecode file {}: {}", path, e))?;

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
        // Try nested format: field.object
        if let Some(bytecode) = json
            .get(field)
            .and_then(|b| b.get("object"))
            .and_then(|o| o.as_str())
        {
            return Ok(bytecode.to_string());
        }

        // Try flat format: field
        if let Some(bytecode) = json.get(field).and_then(|b| b.as_str()) {
            return Ok(bytecode.to_string());
        }
    }

    let bytecode = content.trim().to_string();
    if bytecode.is_empty() {
        return Err(eyre!("Could not find {} in {}", field, path));
    }

    Ok(bytecode)
}

fn load_deployed_bytecode(path: &str) -> Result<String> {
    load_bytecode_from_json(path, "deployedBytecode")
}

fn load_creation_bytecode(path: &str) -> Result<String> {
    load_bytecode_from_json(path, "bytecode")
}

fn parse_u8(s: &str) -> Result<u8> {
    if s.starts_with("0x") {
        u8::from_str_radix(&s[2..], 16).map_err(|e| eyre!("Invalid hex u8: {}", e))
    } else {
        s.parse::<u8>()
            .map_err(|e| eyre!("Invalid decimal u8: {}", e))
    }
}

fn parse_u32(s: &str) -> Result<u32> {
    if s.starts_with("0x") {
        u32::from_str_radix(&s[2..], 16).map_err(|e| eyre!("Invalid hex u32: {}", e))
    } else {
        s.parse::<u32>()
            .map_err(|e| eyre!("Invalid decimal u32: {}", e))
    }
}

fn parse_uint(s: &str) -> Result<alloy::primitives::U256> {
    if s.starts_with("0x") {
        alloy::primitives::U256::from_str_radix(&s[2..], 16)
            .map_err(|e| eyre!("Invalid hex uint: {}", e))
    } else {
        alloy::primitives::U256::from_str_radix(s, 10)
            .map_err(|e| eyre!("Invalid decimal uint: {}", e))
    }
}

/// Convert ConstructorArgs to DynSolValues
fn args_to_dyn_sol_values(
    args: &[ConstructorArg],
    deployed: &HashMap<String, String>,
) -> Result<Vec<alloy::dyn_abi::DynSolValue>> {
    use alloy::dyn_abi::DynSolValue;
    use alloy::primitives::{Address, U256};

    let mut values = Vec::new();

    for arg in args {
        let resolved_value = arg.resolve_value(deployed)?;

        let value = match arg.arg_type.as_str() {
            "address" => {
                let addr: Address = resolved_value
                    .parse()
                    .map_err(|e| eyre!("Invalid address '{}': {}", resolved_value, e))?;
                DynSolValue::Address(addr)
            }
            "uint256" | "uint" => {
                let val = parse_uint(&resolved_value)?;
                DynSolValue::Uint(val, 256)
            }
            "uint32" => {
                let val = parse_u32(&resolved_value)?;
                DynSolValue::Uint(U256::from(val), 32)
            }
            "uint8" => {
                let val = parse_u8(&resolved_value)?;
                DynSolValue::Uint(U256::from(val), 8)
            }
            "string" => DynSolValue::String(resolved_value),
            "bytes" => {
                let data = resolved_value.trim_start_matches("0x");
                let bytes = if data.is_empty() {
                    Vec::new()
                } else {
                    hex::decode(data)
                        .map_err(|e| eyre!("Invalid hex bytes '{}': {}", resolved_value, e))?
                };
                DynSolValue::Bytes(bytes)
            }
            "bool" => {
                let b = resolved_value
                    .parse::<bool>()
                    .map_err(|e| eyre!("Invalid bool value '{}': {}", resolved_value, e))?;
                DynSolValue::Bool(b)
            }
            _ => return Err(eyre!("Unsupported arg type: {}", arg.arg_type)),
        };

        values.push(value);
    }

    Ok(values)
}

/// Encode constructor arguments using alloy with proper ABI encoding
fn encode_constructor_args(
    args: &[ConstructorArg],
    deployed: &HashMap<String, String>,
) -> Result<String> {
    if args.is_empty() {
        return Ok(String::from("0x"));
    }

    let values = args_to_dyn_sol_values(args, deployed)?;
    let tuple = alloy::dyn_abi::DynSolValue::Tuple(values);
    let encoded = tuple
        .abi_encode_sequence()
        .ok_or_else(|| eyre!("Failed to encode constructor arguments"))?;

    Ok(format!("0x{}", hex::encode(encoded)))
}

/// Encode function call with arguments using proper ABI encoding
fn encode_function_call(
    function_sig: &str,
    args: &[ConstructorArg],
    deployed: &HashMap<String, String>,
) -> Result<String> {
    use alloy::primitives::keccak256;

    // Calculate function selector (first 4 bytes of keccak256 hash)
    let selector = &keccak256(function_sig.as_bytes())[0..4];

    // Encode arguments
    let encoded_args = if args.is_empty() {
        String::new()
    } else {
        let values = args_to_dyn_sol_values(args, deployed)?;
        let tuple = alloy::dyn_abi::DynSolValue::Tuple(values);
        let encoded = tuple
            .abi_encode_sequence()
            .ok_or_else(|| eyre!("Failed to encode function arguments"))?;
        hex::encode(encoded)
    };

    Ok(format!("0x{}{}", hex::encode(selector), encoded_args))
}

#[instrument(level = "trace", skip_all)]
async fn rpc_call(
    port: u16,
    client: &Client,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value> {
    let url = format!("http://localhost:{}", port);
    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": 1
    });

    let response: serde_json::Value = client
        .post(&url)
        .json(&request_body)
        .send()
        .await?
        .json()
        .await?;

    Ok(response)
}

#[instrument(level = "trace", skip_all)]
async fn get_nonce(port: u16, client: &Client, address: &str) -> Result<u64> {
    let response = rpc_call(
        port,
        client,
        "eth_getTransactionCount",
        serde_json::json!([address, "latest"]),
    )
    .await?;

    let nonce_hex = response["result"]
        .as_str()
        .ok_or_else(|| eyre!("Invalid nonce response"))?
        .trim_start_matches("0x");

    Ok(u64::from_str_radix(nonce_hex, 16)?)
}

#[instrument(level = "trace", skip_all)]
async fn execute_transaction(
    port: u16,
    client: &Client,
    from: &str,
    to: Option<&str>,
    data: &str,
    nonce: u64,
) -> Result<String> {
    let mut params = serde_json::json!({
        "from": from,
        "data": data,
        "nonce": format!("0x{:x}", nonce),
        "gas": "0x500000",
    });

    if let Some(to_addr) = to {
        params["to"] = serde_json::json!(to_addr);
    }

    let res = rpc_call(
        port,
        client,
        "eth_sendTransaction",
        serde_json::json!([params]),
    )
    .await?;

    if let Some(result) = res.get("result").and_then(|r| r.as_str()) {
        return Ok(result.to_string());
    }
    if let Some(error) = res.get("error") {
        return Err(eyre!("{error}"));
    }
    Err(eyre!("unexpected response: {res}"))
}

#[instrument(level = "trace", skip_all)]
async fn get_transaction_receipt(
    port: u16,
    client: &Client,
    tx_hash: &str,
) -> Result<Option<String>> {
    let response = rpc_call(
        port,
        client,
        "eth_getTransactionReceipt",
        serde_json::json!([tx_hash]),
    )
    .await?;

    if let Some(receipt) = response.get("result") {
        if receipt.is_null() {
            return Ok(None);
        }
        if let Some(contract_address) = receipt.get("contractAddress").and_then(|a| a.as_str()) {
            return Ok(Some(contract_address.to_string()));
        }
    }

    Ok(None)
}

struct AnvilImpersonator<'a> {
    port: u16,
    client: &'a Client,
    address: &'a str,
}

impl<'a> AnvilImpersonator<'a> {
    async fn new(port: u16, client: &'a Client, address: &'a str) -> Result<Self> {
        rpc_call(
            port,
            client,
            "anvil_impersonateAccount",
            serde_json::json!([address]),
        )
        .await?;
        Ok(Self {
            port,
            client,
            address,
        })
    }
}

impl<'a> Drop for AnvilImpersonator<'a> {
    fn drop(&mut self) {
        // Best effort cleanup - ignore errors
        let port = self.port;
        let client = self.client.clone();
        let address = self.address.to_string();

        tokio::spawn(async move {
            let _ = rpc_call(
                port,
                &client,
                "anvil_stopImpersonatingAccount",
                serde_json::json!([address]),
            )
            .await;
        });
    }
}

#[instrument(level = "trace", skip_all)]
async fn deploy_contracts(
    port: u16,
    config: &ChainConfig,
    deployed: &HashMap<String, String>,
) -> Result<HashMap<String, String>> {
    let client = Client::new();
    let mut deployed_addresses = deployed.clone();

    // First, collect addresses from config (contracts with explicit address)
    for contract in &config.contracts {
        if let (Some(name), Some(address)) = (&contract.name, &contract.address) {
            deployed_addresses.insert(name.clone(), address.clone());
            info!("Pre-registered contract '{}' at {}", name, address);
        }
    }

    let _impersonator = AnvilImpersonator::new(port, &client, OWNER_ADDRESS).await?;
    let mut nonce = get_nonce(port, &client, OWNER_ADDRESS).await?;

    // Deploy contracts sequentially
    for contract in &config.contracts {
        if let Some(json_path) = &contract.contract_json_path {
            let name = contract.name.as_deref().unwrap_or("unnamed");

            // Check if contract has predefined address and code already exists
            if let Some(address) = &contract.address {
                match check_code(port, address).await {
                    Ok(true) => {
                        info!(
                            "Skipping deploy for '{}': code already present at {}",
                            name, address
                        );
                        // Ensure address is registered
                        if let Some(name) = &contract.name {
                            deployed_addresses.insert(name.clone(), address.clone());
                        }
                        continue;
                    }
                    Ok(false) => {
                        info!(
                            "Contract '{}' has predefined address {} but no code yet, proceeding with deploy",
                            name, address
                        );
                    }
                    Err(e) => {
                        debug!("Failed to check code for '{}' at {}: {}", name, address, e);
                    }
                }
            }

            let mut bytecode = load_creation_bytecode(json_path)?;

            // Append constructor args if any
            if !contract.constructor_args.is_empty() {
                let encoded_args =
                    encode_constructor_args(&contract.constructor_args, &deployed_addresses)?;
                bytecode = format!(
                    "0x{}{}",
                    bytecode.trim_start_matches("0x"),
                    encoded_args.trim_start_matches("0x")
                );
            }

            info!(
                "Deploying contract '{}' with bytecode length: {}",
                name,
                bytecode.len()
            );

            match execute_transaction(port, &client, OWNER_ADDRESS, None, &bytecode, nonce).await {
                Ok(tx_hash) => {
                    info!("Deployment tx for '{}': {}", name, tx_hash);

                    // Wait for receipt
                    for _ in 0..10 {
                        sleep(Duration::from_millis(100)).await;
                        if let Ok(Some(contract_address)) =
                            get_transaction_receipt(port, &client, &tx_hash).await
                        {
                            info!("Contract '{}' deployed at: {}", name, contract_address);
                            if let Some(name) = &contract.name {
                                deployed_addresses.insert(name.clone(), contract_address);
                            }
                            break;
                        }
                    }

                    // Increment nonce only after successful deployment
                    nonce += 1;
                }
                Err(e) => {
                    error!("Failed to deploy contract '{}': {}", name, e);
                    // Still increment nonce as transaction was attempted
                    nonce += 1;
                }
            }
        }
    }

    Ok(deployed_addresses)
}

#[instrument(level = "trace", skip_all)]
async fn execute_config_transactions(
    port: u16,
    config: &ChainConfig,
    deployed: &HashMap<String, String>,
) -> Result<()> {
    if config.transactions.is_empty() {
        return Ok(());
    }

    info!(
        "Found {} configured transactions",
        config.transactions.len()
    );

    let client = Client::new();
    let _impersonator = AnvilImpersonator::new(port, &client, OWNER_ADDRESS).await?;
    let mut nonce = get_nonce(port, &client, OWNER_ADDRESS).await?;

    for tx_config in &config.transactions {
        let name = tx_config.name.as_deref().unwrap_or("unnamed");

        // Resolve target address
        let target = if let Some(ref_name) = tx_config.target.strip_prefix('#') {
            deployed.get(ref_name).cloned().ok_or_else(|| {
                eyre!(
                    "Reference to unknown contract in transaction target: #{}",
                    ref_name
                )
            })?
        } else {
            tx_config.target.clone()
        };

        // Build transaction data
        let data = if let Some(inline_data) = &tx_config.data {
            inline_data.clone()
        } else if let Some(function_sig) = &tx_config.function_signature {
            encode_function_call(function_sig, &tx_config.args, deployed)?
        } else {
            return Err(eyre!(
                "Transaction '{}' must have either 'data' or 'function_signature'",
                name
            ));
        };

        info!("Executing transaction '{}' to {}", name, target);

        match execute_transaction(port, &client, OWNER_ADDRESS, Some(&target), &data, nonce).await {
            Ok(tx_hash) => info!("Transaction '{}' sent: {}", name, tx_hash),
            Err(e) => info!("Transaction '{}' failed: {}", name, e),
        }

        nonce += 1;
    }

    Ok(())
}

#[instrument(level = "trace", skip_all)]
async fn mint_test_tbas(port: u16, addresses: &mut ContractAddresses) -> Result<()> {
    info!("Minting test TBAs...");

    let Some(ref permissioned_minter) = addresses.hyper_permissioned_minter else {
        info!("Skipping TBA minting: hyper_permissioned_minter not deployed");
        return Ok(());
    };

    let tba_of_calldata = format!(
        "{}{}",
        TBA_OF_SELECTOR, "0000000000000000000000000000000000000000000000000000000000000000"
    );
    let zeroth_tba_result =
        call_contract(port, &addresses.hypermap_proxy, &tba_of_calldata).await?;
    info!("zeroth_tba_result: {}", zeroth_tba_result);

    // Extract address from result (last 20 bytes / 40 hex chars)
    let zeroth_tba = if zeroth_tba_result.len() >= 42 {
        format!("0x{}", &zeroth_tba_result[zeroth_tba_result.len() - 40..])
    } else {
        return Err(eyre!("Hypermap is not initialized"));
    };

    info!("Resolved zeroth_tba from hypermap: {}", zeroth_tba);
    addresses.zeroth_tba = Some(zeroth_tba.clone());

    let client = Client::new();
    let _impersonator = AnvilImpersonator::new(port, &client, OWNER_ADDRESS).await?;
    let nonce = get_nonce(port, &client, OWNER_ADDRESS).await?;

    // Build mint calldata: mint(address to, bytes label, bytes initialization, address implementation)
    let mint_args = vec![
        ConstructorArg {
            arg_type: "address".to_string(),
            value: OWNER_ADDRESS.to_string(),
        },
        ConstructorArg {
            arg_type: "bytes".to_string(),
            value: DOT_OS_LABAL_HEX.to_string(),
        },
        ConstructorArg {
            arg_type: "bytes".to_string(),
            value: "0x".to_string(),
        },
        ConstructorArg {
            arg_type: "address".to_string(),
            value: permissioned_minter.clone(),
        },
    ];

    let deployed_map = HashMap::new();
    let mint_calldata = encode_function_call(
        "mint(address,bytes,bytes,address)",
        &mint_args,
        &deployed_map,
    )?;

    info!("Mint calldata: {}", mint_calldata);

    // Build execute calldata: execute(address,uint256,bytes,uint8)
    let execute_args = vec![
        ConstructorArg {
            arg_type: "address".to_string(),
            value: addresses.hypermap_proxy.clone(),
        },
        ConstructorArg {
            arg_type: "uint256".to_string(),
            value: "0".to_string(),
        },
        ConstructorArg {
            arg_type: "bytes".to_string(),
            value: mint_calldata.clone(),
        },
        ConstructorArg {
            arg_type: "uint8".to_string(),
            value: "0".to_string(),
        },
    ];

    let execute_calldata = encode_function_call(
        "execute(address,uint256,bytes,uint8)",
        &execute_args,
        &deployed_map,
    )?;

    info!("Execute calldata: {}", execute_calldata);

    // Send transaction to zeroth_tba
    match execute_transaction(
        port,
        &client,
        OWNER_ADDRESS,
        Some(&zeroth_tba),
        &execute_calldata,
        nonce,
    )
    .await
    {
        Ok(tx_hash) => {
            info!("Mint TBA transaction sent: {}", tx_hash);

            sleep(Duration::from_millis(200)).await;

            let tba_of_calldata = format!("{}{}", TBA_OF_SELECTOR, &DOT_OS_TOKEN_ID[2..]);

            if let Ok(dot_os_tba_result) =
                call_contract(port, &addresses.hypermap_proxy, &tba_of_calldata).await
            {
                info!("dot_os_tba_result: {}", dot_os_tba_result);
                if dot_os_tba_result.len() >= 42 {
                    let dot_os_tba =
                        format!("0x{}", &dot_os_tba_result[dot_os_tba_result.len() - 40..]);
                    info!("Minted dot_os_tba: {}", dot_os_tba);
                    addresses.dot_os_tba = Some(dot_os_tba);
                }
            }
        }
        Err(e) => {
            error!("Mint TBA transaction failed: {}", e);
        }
    }

    info!("Test TBAs minted successfully");
    Ok(())
}

#[instrument(level = "trace", skip_all)]
async fn apply_config_contracts(
    port: u16,
    config: &ChainConfig,
    deployed: &HashMap<String, String>,
) -> Result<()> {
    let client = Client::new();

    // Only process contracts with explicit address (not deployed via contract_json_path)
    for contract in &config.contracts {
        // Skip if this is a deployment contract
        if contract.contract_json_path.is_some() {
            continue;
        }

        let Some(address) = &contract.address else {
            continue;
        };

        let bytecode = if let Some(inline_bytecode) = &contract.bytecode {
            Some(inline_bytecode.clone())
        } else if let Some(deployed_path) = &contract.deployed_bytecode_path {
            Some(load_deployed_bytecode(deployed_path)?)
        } else {
            None
        };

        if let Some(bytecode) = bytecode {
            // Check if code already exists before overwriting
            match check_code(port, address).await {
                Ok(true) => {
                    let name_info = contract.name.as_deref().unwrap_or("unnamed");
                    info!(
                        "Skipping setCode for '{}' at {}: code already present",
                        name_info, address
                    );
                }
                Ok(false) => {
                    rpc_call(
                        port,
                        &client,
                        "anvil_setCode",
                        serde_json::json!([address, bytecode.trim()]),
                    )
                    .await?;

                    let name_info = contract.name.as_deref().unwrap_or("unnamed");
                    info!("Set bytecode for contract '{}' at {}", name_info, address);
                }
                Err(e) => {
                    debug!("Failed to check code at {}: {}", address, e);
                }
            }
        }

        // Apply storage with reference resolution
        for (slot, value) in &contract.storage {
            let normalized_slot = normalize_slot(slot);
            let hex_value = value.to_hex_string(deployed)?;

            rpc_call(
                port,
                &client,
                "anvil_setStorageAt",
                serde_json::json!([address, normalized_slot, hex_value]),
            )
            .await?;

            debug!("Set storage slot {} for {} to {}", slot, address, hex_value);
        }
    }

    Ok(())
}

#[instrument(level = "trace", skip_all)]
async fn check_code(port: u16, address: &str) -> Result<bool> {
    let client = Client::new();

    let response = rpc_call(
        port,
        &client,
        "eth_getCode",
        serde_json::json!([address, "latest"]),
    )
    .await?;
    let code = response["result"].as_str().unwrap_or("0x");
    Ok(code != "0x")
}

async fn process_configs(
    port: u16,
    default_config: Option<&ChainConfig>,
    custom_config: Option<&ChainConfig>,
) -> Result<HashMap<String, String>> {
    let mut deployed_addresses = HashMap::new();

    // Step 1: Collect ALL pre-registered addresses from both configs FIRST
    // This ensures custom config can reference default config contracts
    if let Some(config) = default_config {
        for contract in &config.contracts {
            if let (Some(name), Some(address)) = (&contract.name, &contract.address) {
                deployed_addresses.insert(name.clone(), address.clone());
                debug!("Pre-registered from default config: {} = {}", name, address);
            }
        }
    }
    if let Some(config) = custom_config {
        for contract in &config.contracts {
            if let (Some(name), Some(address)) = (&contract.name, &contract.address) {
                deployed_addresses.insert(name.clone(), address.clone());
                debug!("Pre-registered from custom config: {} = {}", name, address);
            }
        }
    }

    // Step 2: Deploy contracts from default config first
    // Now deployed_addresses contains all pre-registered addresses
    if let Some(config) = default_config {
        deployed_addresses.extend(deploy_contracts(port, config, &deployed_addresses).await?);
    }

    // Step 3: Deploy contracts from custom config (can reference default config contracts)
    if let Some(config) = custom_config {
        deployed_addresses.extend(deploy_contracts(port, config, &deployed_addresses).await?);
    }

    // Step 4: Apply bytecode at known addresses
    if let Some(config) = default_config {
        apply_config_contracts(port, config, &deployed_addresses).await?;
    }
    if let Some(config) = custom_config {
        apply_config_contracts(port, config, &deployed_addresses).await?;
    }

    // Step 5: Execute config transactions
    if let Some(config) = default_config {
        execute_config_transactions(port, config, &deployed_addresses).await?;
    }
    if let Some(config) = custom_config {
        execute_config_transactions(port, config, &deployed_addresses).await?;
    }

    Ok(deployed_addresses)
}

#[instrument(level = "trace", skip_all)]
pub async fn start_chain(
    port: u16,
    recv_kill: BroadcastRecvBool,
    verbose: bool,
    tracing: bool,
    config_path: Option<PathBuf>,
) -> Result<Option<Child>> {
    start_chain_with_config(port, recv_kill, verbose, tracing, config_path).await
}

#[instrument(level = "trace", skip_all)]
pub async fn start_chain_with_config(
    port: u16,
    mut recv_kill: BroadcastRecvBool,
    verbose: bool,
    tracing: bool,
    custom_config_path: Option<PathBuf>,
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

    // Default config should always load successfully (it's embedded)
    let default_config = load_default_config()?;

    // Custom config is optional
    let custom_config = if let Some(path) = custom_config_path {
        load_config(&path)?
    } else {
        None
    };

    let configs = ConfigSet::new(&default_config, custom_config.as_ref());

    info!("Checking for Anvil on port {}...", port);
    if wait_for_anvil(port, 1, None).await.is_ok() {
        if !check_code(port, HYPERMAP).await? {
            let deployed_addresses = configs.process(port).await?;
            let mut addresses =
                ContractAddresses::from_config(configs.active(), &deployed_addresses)?;
            mint_test_tbas(port, &mut addresses).await?;
            addresses.print_summary();
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

    let deployed_addresses = match configs.process(port).await {
        Ok(addrs) => addrs,
        Err(e) => {
            let _ = child.kill();
            return Err(e.wrap_err("Failed to process configs"));
        }
    };

    let mut addresses = ContractAddresses::from_config(configs.active(), &deployed_addresses)?;

    if let Err(e) = mint_test_tbas(port, &mut addresses).await {
        let _ = child.kill();
        return Err(e.wrap_err("Failed to mint test TBAs"));
    }

    if let Err(e) = verify_contracts(port, &addresses).await {
        let _ = child.kill();
        return Err(e.wrap_err("Contract verification failed"));
    }

    addresses.print_summary();

    Ok(Some(child))
}

#[instrument(level = "trace", skip_all)]
async fn wait_for_anvil(
    port: u16,
    max_attempts: u16,
    mut recv_kill: Option<BroadcastRecvBool>,
) -> Result<()> {
    let client = Client::new();

    for _ in 0..max_attempts {
        if let Ok(response) =
            rpc_call(port, &client, "eth_blockNumber", serde_json::json!([])).await
        {
            if let Some(block_number) = response["result"].as_str() {
                if block_number.starts_with("0x") {
                    info!("Anvil is ready on port {}.", port);
                    return Ok(());
                }
            }
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
pub async fn call_contract(port: u16, target: &str, data: &str) -> Result<String> {
    let client = Client::new();
    let result = rpc_call(
        port,
        &client,
        "eth_call",
        serde_json::json!([{"to": target, "data": data}, "latest"]),
    )
    .await?;

    if let Some(output) = result.get("result").and_then(|r| r.as_str()) {
        return Ok(output.to_string());
    }

    if let Some(error) = result.get("error") {
        return Err(eyre!("Contract call failed: {}", error));
    }

    Err(eyre!("Unexpected response: {}", result))
}

#[instrument(level = "trace", skip_all)]
pub async fn verify_contracts(port: u16, addresses: &ContractAddresses) -> Result<()> {
    info!("Verifying deployed contracts...");

    match call_contract(port, &addresses.hypermap_proxy, SYMBOL_CALLDATA).await {
        Ok(result) => {
            if result == "0x" || result.is_empty() {
                return Err(eyre!("Hypermap symbol() returned empty result"));
            }
            info!("Hypermap symbol: {}", result);
        }
        Err(e) => {
            return Err(e.wrap_err("Failed to verify Hypermap contract"));
        }
    }

    // Verify HyperAccount - entryPoint()
    let entry_calldata = "0xb0d691fe"; // entryPoint()
    match call_contract(port, &addresses.hyperaccount, entry_calldata).await {
        Ok(result) => {
            let result_trimmed = result.trim_start_matches("0x000000000000000000000000");
            if result == "0x"
                || result.is_empty()
                || result_trimmed == "0"
                || result_trimmed.len() < 40
            {
                return Err(eyre!("HyperAccount entryPoint() returned zero address"));
            }
            info!("HyperAccount entryPoint: {}", result);
        }
        Err(e) => {
            return Err(e.wrap_err("Failed to verify HyperAccount contract"));
        }
    }

    match call_contract(port, &addresses.erc6551registry, VERIFY_REGISTRY_CALLDATA).await {
        Ok(result) => {
            if result == "0x" || result.is_empty() {
                return Err(eyre!("ERC6551Registry account() returned empty result"));
            }
            info!("ERC6551Registry account: {}", result);
        }
        Err(e) => {
            return Err(e.wrap_err("Failed to verify ERC6551Registry contract"));
        }
    }

    match call_contract(port, &addresses.multicall, VERIFY_MULTICALL_CALLDATA).await {
        Ok(result) => {
            if result == "0x" || result.is_empty() {
                return Err(eyre!("Multicall3 getBasefee() returned empty result"));
            }
            info!("Multicall3 basefee: {}", result);
        }
        Err(e) => {
            return Err(e.wrap_err("Failed to verify Multicall3 contract"));
        }
    }

    // Verify Create2 factory - just check code exists
    match get_code(port, &addresses.create2).await {
        Ok(code) => {
            if code == "0x" || code.is_empty() {
                return Err(eyre!("Create2 factory has no code deployed"));
            }
            info!("Create2 factory deployed (code length: {})", code.len());
        }
        Err(e) => {
            return Err(e.wrap_err("Failed to verify Create2 factory"));
        }
    }

    info!("All contracts verified successfully");
    Ok(())
}

// Helper function to get contract code
async fn get_code(port: u16, address: &str) -> Result<String> {
    let client = reqwest::Client::new();
    let rpc_url = format!("http://127.0.0.1:{}", port);

    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getCode",
        "params": [address, "latest"],
        "id": 1
    });

    let response = client
        .post(&rpc_url)
        .json(&payload)
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;

    if let Some(error) = response.get("error") {
        return Err(eyre!("RPC error: {}", error));
    }

    response
        .get("result")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| eyre!("Invalid response format"))
}

#[instrument(level = "trace", skip_all)]
pub async fn execute(
    port: u16,
    verbose: bool,
    tracing: bool,
    custom_config_path: Option<PathBuf>,
) -> Result<()> {
    let (send_to_cleanup, mut recv_in_cleanup) = tokio::sync::mpsc::unbounded_channel();
    let (send_to_kill, _recv_kill) = tokio::sync::broadcast::channel(1);
    let recv_kill_in_cos = send_to_kill.subscribe();

    let handle_signals = tokio::spawn(cleanup_on_signal(send_to_cleanup.clone(), recv_kill_in_cos));

    let recv_kill_in_start_chain = send_to_kill.subscribe();
    let child = start_chain_with_config(
        port,
        recv_kill_in_start_chain,
        verbose,
        tracing,
        custom_config_path,
    )
    .await?;

    // If child is None, it means we connected to an existing Anvil instance
    // In this case, we don't need to manage the process lifecycle
    let Some(mut child) = child else {
        info!(
            "Connected to existing Anvil instance on port {}, initialization complete",
            port
        );
        return Ok(());
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
