use alloy::{
    primitives::{Address, FixedBytes},
    providers::{network::TransactionBuilder, Provider, ProviderBuilder},
    rpc::{client::WsConnect, types::eth::TransactionRequest},
    signers::wallet::LocalWallet,
    sol_types::SolCall,
};

use color_eyre::eyre::{eyre, Result};
use std::net::TcpListener;
use std::process::{Child, Command};
use std::str::FromStr;

pub mod register;

use register::{
    dns_encode_fqdn, encode_namehash,
    RegisterHelpers::{registerCall, setAllIpCall, setKeyCall},
};

pub async fn start_chain_and_register(
    chain_port: u16,
    name: &str,
    node_port: u16,
    pubkey: &str,
) -> Result<Child> {
    // todo add optional args
    // fetch json from github link? patch anvil to accept state in builder?

    let process = start_chain(chain_port);
    // if error, log it somewhere

    let wallet = LocalWallet::from_str(
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
    )?;

    let endpoint = format!("ws://localhost:{}", chain_port);
    let provider = ProviderBuilder::new()
        .on_ws(WsConnect::new(endpoint))
        .await?;

    let fqdn = dns_encode_fqdn(name);
    let namehash = encode_namehash(name);

    println!(
        "hex version of fqdn and namehash: {:?}, {:?}",
        hex::encode(fqdn.clone()),
        hex::encode(namehash)
    );
    let ip: u128 = 0x7F000001; // localhost IP (127.0.0.1)

    let set_ip = setAllIpCall {
        _node: namehash.into(),
        _ip: ip,
        _ws: node_port,
        _wt: 0,
        _tcp: 0,
        _udp: 0,
    }
    .abi_encode();

    println!("set ip hex encoded: {:?}", hex::encode(set_ip.clone()));
    println!("pubkey: {:?}", pubkey);
    println!(
        "pubkey parsed and hexxed: {:?}",
        hex::encode(pubkey.parse::<FixedBytes<32>>().unwrap())
    );
    let set_key = setKeyCall {
        _node: namehash.into(),
        _key: pubkey.parse()?,
    }
    .abi_encode();

    println!("set key hex encoded: {:?}", hex::encode(set_key.clone()));

    let register = registerCall {
        _name: fqdn.into(),
        _to: Address::from_str("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266")?,
        _data: vec![set_ip.into(), set_key.into()],
    };

    let dotdev = Address::from_str("0xDc64a140Aa3E981100a9becA4E685f962f0cF6C9")?;

    let tx = TransactionRequest::default()
        .with_from(wallet.address())
        .with_to(dotdev)
        .with_call(&register);

    println!("making tx req: {:?}", tx);

    let x = provider.send_transaction(tx).await?.watch().await;
    println!("got x tx {:?}", x);
    process
}

pub fn start_chain(port: u16) -> Result<Child> {
    let child = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(_) => {
            let child = Command::new("anvil")
                .arg("--port")
                .arg(port.to_string())
                .stdout(std::process::Stdio::piped())
                .spawn()?;
            Ok(child)
        }
        Err(e) => Err(eyre!("Port {} is already in use: {}", port, e)),
    };

    // TODO: read stdout to know when anvil is ready instead.
    std::thread::sleep(std::time::Duration::from_millis(100));
    child
}

/// kit chain, alias to anvil
pub async fn execute(port: u16) -> Result<()> {
    Command::new("anvil")
        .arg("--port")
        .arg(port.to_string())
        .stdout(std::process::Stdio::piped())
        // .arg("--load-state")
        // .arg("./kinostate.json")
        .spawn()?;

    Ok(())
}
