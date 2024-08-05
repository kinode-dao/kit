use std::path::{Path, PathBuf};
use std::str::FromStr;

use alloy::{
    network::{eip2718::Encodable2718, EthereumWallet, TransactionBuilder},
    primitives::{keccak256, Address, Bytes, B256, U256},
    providers::{Provider, ProviderBuilder, RootProvider},
    pubsub::PubSubFrontend,
    rpc::{
        client::WsConnect,
        types::eth::{TransactionInput, TransactionRequest},
    },
    signers::local::LocalSigner,
};
use alloy_sol_macro::sol;
use alloy_sol_types::SolCall;
use color_eyre::eyre::{eyre, Result};
use fs_err as fs;
use tracing::{info, instrument};

use crate::build::{download_file, read_metadata};
use crate::start_package::{make_pkg_publisher, zip_pkg};

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

const KIMAP_ADDRESS: &str = "0xEce71a05B36CA55B895427cD9a440eEF7Cf3669D";
const MULTICALL_ADDRESS: &str = "0xcA11bde05977b3631167028862bE2a173976CA11";
const KINO_ACCOUNT_IMPL: &str = "0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0";
const FAKE_DOTDEV_TBA: &str = "0x69C30C0Cf0e9726f9eEF50bb74FA32711fA0B02D";

#[instrument(level = "trace", skip_all)]
fn calculate_metadata_hash(package_dir: &Path) -> Result<String> {
    let metadata_text = fs::read_to_string(package_dir.join("metadata.json"))?;
    let hash = keccak256(metadata_text.as_bytes());
    Ok(format!("0x{}", hex::encode(hash)))
}

#[instrument(level = "trace", skip_all)]
fn read_keystore(keystore_path: &Path) -> Result<(Address, EthereumWallet)> {
    let password = rpassword::prompt_password("Enter password: ")?;
    let signer = LocalSigner::decrypt_keystore(keystore_path, password)?;
    let address = signer.address();
    let wallet = EthereumWallet::from(signer);
    Ok((address, wallet))
}

fn namehash(name: &str) -> [u8; 32] {
    let mut node = B256::default();

    if name.is_empty() {
        return node.into();
    }
    let mut labels: Vec<&str> = name.split(".").collect();
    labels.reverse();
    for label in labels.iter() {
        let label_hash = keccak256(label.as_bytes());
        node = keccak256([node, label_hash].concat());
    }
    node.into()
}

#[instrument(level = "trace", skip_all)]
pub async fn execute(
    package_dir: &Path,
    metadata_uri: &str,
    keystore_path: &Path,
    fakechain_port: &u16,
) -> Result<()> {
    if !package_dir.join("pkg").exists() {
        return Err(eyre!(
            "Required `pkg/` dir not found within given input dir {:?} (or cwd, if none given). Please re-run targeting a package.",
            package_dir,
        ));
    }

    let metadata = read_metadata(package_dir)?;
    let name = metadata.name.clone().unwrap_or_default();

    let remote_metadata_dir = PathBuf::from(format!("/tmp/kinode-kit-cache/{name}"));
    if !remote_metadata_dir.exists() {
        fs::create_dir_all(&remote_metadata_dir)?;
    }
    let remote_metadata_path = remote_metadata_dir.join("metadata.json");
    download_file(metadata_uri, &remote_metadata_path).await?;
    let remote_metadata = read_metadata(&remote_metadata_dir)?;

    // TODO: add derive(PartialEq) to Erc721
    if serde_json::to_string(&metadata)? != serde_json::to_string(&remote_metadata)? {
        let local_path = package_dir
            .join("metadata.json")
            .canonicalize()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .unwrap_or_default();
        return Err(eyre!(
            "\x1B]8;;file://{}\x1B\\Local\x1B]8;;\x1B\\ and \x1B]8;;{}\x1B\\remote\x1B]8;;\x1B\\ metadata do not match",
            local_path,
            metadata_uri,
        ));
    }
    let metadata_hash = calculate_metadata_hash(package_dir)?;

    let pkg_publisher = make_pkg_publisher(&metadata);
    let (_, pkg_hash) = zip_pkg(package_dir, &pkg_publisher)?;
    let current_version = &metadata.properties.current_version;
    let expected_pkg_hash = metadata
        .properties
        .code_hashes
        .get(current_version)
        .cloned()
        .unwrap_or_default();
    if pkg_hash != expected_pkg_hash {
        return Err(eyre!(
            "Zipped pkg hashes to '{}' not '{}' as expected for current_version {} based on published metadata at \x1B]8;;{}\x1B\\{}\x1B]8;;\x1B\\",
            pkg_hash,
            expected_pkg_hash,
            current_version,
            metadata_uri,
            metadata_uri,
        ));
    }

    let (wallet_address, wallet) = read_keystore(keystore_path)?;

    let endpoint = format!("ws://localhost:{}", fakechain_port);
    let ws = WsConnect::new(endpoint);
    let provider: RootProvider<PubSubFrontend> = ProviderBuilder::default().on_ws(ws).await?;

    let kimap = Address::from_str(KIMAP_ADDRESS)?;
    let multicall_address = Address::from_str(MULTICALL_ADDRESS)?;
    let fakedotdev_tba = Address::from_str(FAKE_DOTDEV_TBA)?;
    let kino_account_impl = Address::from_str(KINO_ACCOUNT_IMPL)?;

    // Create metadata calls
    let metadata_uri_call = noteCall {
        note: "~metadata-uri".into(),
        data: metadata_uri.to_string().into(),
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

    // Check if the app already exists
    let app_node = format!("{}.{}", name, wallet_address);
    let app_node = namehash(&app_node);

    let get_tx = TransactionRequest::default().to(kimap).input(
        getCall {
            node: app_node.into(),
        }
        .abi_encode()
        .into(),
    );

    let get_call = provider.call(&get_tx).await?;
    let decoded = getCall::abi_decode_returns(&get_call, false)?;

    let tba = decoded.tba;
    let owner = decoded.owner;
    let _data = decoded.data;

    let is_update = tba != Address::default() && owner == wallet_address;

    let init_call = executeCall {
        to: multicall_address,
        value: U256::from(0),
        data: notes_multicall.into(),
        operation: 1,
    }
    .abi_encode();

    //let create_app_tba_call = mintCall {
    //    who: wallet_address,
    //    label: name.clone().into(),
    //    initialization: init_call.into(),
    //    erc721Data: Bytes::default(),
    //    implementation: kino_account_impl,
    //}
    //.abi_encode();

    //let create_app_call = executeCall {
    //    to: kimap,
    //    value: U256::from(0),
    //    data: create_app_tba_call.into(),
    //    operation: 0,
    //}
    //.abi_encode();
    let create_app_call = if is_update {
        executeCall {
            to: kimap,
            value: U256::from(0),
            data: init_call.into(),
            operation: 1,
        }
        .abi_encode()
    } else {
        mintCall {
            who: wallet_address,
            label: name.clone().into(),
            initialization: init_call.into(),
            erc721Data: Bytes::default(),
            implementation: kino_account_impl,
        }
        .abi_encode()
    };

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

    info!(
        "{name} published successfully; tx hash {:?}",
        tx_hash.tx_hash()
    );
    Ok(())
}
