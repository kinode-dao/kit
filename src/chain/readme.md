# Hyperware Local Chain Setup

A local Ethereum development chain setup tool for Hyperware development, featuring automated contract deployment and configuration.

## Overview

This tool provides a streamlined way to start a local Anvil chain with pre-configured Hyperware contracts. It supports:

* Automatic contract deployment from TOML configuration

* Storage slot manipulation

* Transaction execution

* NFT minting for testing

* Custom contract configurations

---

## Quick Start

### Start Chain with Default Configuration

```bash
kit chain
```

This starts Anvil on port 8545 with the default Hyperware contract setup.

### Start Chain with Custom Configuration

```bash
kit chain --config path/to/Contracts.toml
```

### Command Options

* `-p, --port <PORT>` - Port to run the chain on (default: 8545)

* `-v, --verbose` - Output stdout and stderr

* `-t, --tracing` - Enable tracing/steps-tracing

* `--config <PATH>` - Path to contracts config file (TOML format)

---

## Preparing Contract Artifacts (JSON files)

Before running the chain, you need to have compiled JSON artifacts for all contracts referenced in your TOML configuration.

### How to Get the JSON Files

1. Navigate to your **protocol** project directory (where the Solidity contracts are located).

2. Run the build command using **[Foundry](https://book.getfoundry.sh/)**:

  ```bash
  forge build
  ```

3. The resulting `.json` artifacts for each contract will be located in the `out/` directory.\
  Example paths:

  ```
  ./out/MyContract.sol/MyContract.json
  ./out/Proxy.sol/Proxy.json
  ```

4. Use these paths in your TOML configuration under `contract_json_path` or `deployed_bytecode_path`.

Example TOML snippet:

```toml
[[contracts]]
name = "my-contract"
contract_json_path = "./out/MyContract.sol/MyContract.json"
```

---

## Configuration File Format

The configuration file uses TOML format with two main sections: `[[contracts]]` and `[[transactions]]`.

### Contract Configuration

#### Deploy a New Contract

```toml
[[contracts]]
name = "my-contract"
contract_json_path = "./path/to/Contract.json"
constructor_args = [
    { type = "address", value = "0x..." },
    { type = "uint256", value = "1000" }
]
```

#### Set Bytecode at Known Address

```toml
[[contracts]]
name = "erc6551registry"
address = "0x000000006551c19487814612e58FE06813775758"
bytecode = "0x608060405234801561001057600080fd5b50..."
```

#### Load Bytecode from JSON Artifact

```toml
[[contracts]]
name = "hypermap-proxy"
address = "0x000000000044C6B8Cb4d8f0F889a3E47664EAeda"
deployed_bytecode_path = "./contracts/ERC1967Proxy.json"
```

#### Set Storage Slots

```toml
[[contracts]]
name = "hypermap-proxy"
address = "0x000000000044C6B8Cb4d8f0F889a3E47664EAeda"
deployed_bytecode_path = "./contracts/ERC1967Proxy.json"

[contracts.storage]
# Implementation slot for ERC1967 proxy
"0x360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc" = "#hypermap-impl"
# Direct value
"0x0" = "0x1234567890abcdef"
# Numeric value
"0x1" = 42
```

### Transaction Configuration

Execute transactions after contract deployment:

```toml
[[transactions]]
name = "initialize-hypermap"
target = "#hypermap-proxy"
function_signature = "initialize(address)"
args = [
    { type = "address", value = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266" }
]
```

#### Using Raw Data

```toml
[[transactions]]
name = "custom-call"
target = "0x..."
data = "0x12345678..."
```

---

## Reference System

Use `#contract-name` to reference deployed contracts:

```toml
[[contracts]]
name = "hyperaccount"
contract_json_path = "./contracts/HyperAccount.json"
constructor_args = [
    { type = "address", value = "#hypermap-proxy" }
]

[[transactions]]
name = "setup"
target = "#hyperaccount"
function_signature = "setRegistry(address)"
args = [
    { type = "address", value = "#erc6551registry" }
]
```

---

## Supported Types

### Constructor/Function Arguments

* `address` - Ethereum address

* `uint256`, `uint` - 256-bit unsigned integer

* `uint32` - 32-bit unsigned integer

* `uint8` - 8-bit unsigned integer

* `string` - String value

* `bytes` - Hex-encoded bytes

* `bool` - Boolean value

### Storage Values

* String (hex address): `"0x1234..."`

* Reference: `"#contract-name"`

* Number: `42`

---

## Default Configuration

The tool includes a default configuration (`Contracts.toml`) with:

* ERC6551 Registry

* Multicall3

* CREATE2 Factory

* Hypermap (proxy + implementation)

* HyperAccount

* HyperAccount Minters (standard, 9-char commit, permissioned)

---

## Examples

### Minimal Setup

```toml
[[contracts]]
name = "my-token"
contract_json_path = "./out/MyToken.sol/MyToken.json"
constructor_args = [
    { type = "string", value = "MyToken" },
    { type = "string", value = "MTK" }
]
```

### Proxy Pattern

```toml
[[contracts]]
name = "implementation"
contract_json_path = "./out/Implementation.sol/Implementation.json"

[[contracts]]
name = "proxy"
address = "0x..."
deployed_bytecode_path = "./out/Proxy.sol/Proxy.json"

[contracts.storage]
# ERC1967 implementation slot
"0x360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc" = "#implementation"

[[transactions]]
name = "initialize"
target = "#proxy"
function_signature = "initialize(address,uint256)"
args = [
    { type = "address", value = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266" },
    { type = "uint256", value = "1000000" }
]
```

### Multiple Contract Deployment

```toml
[[contracts]]
name = "token"
contract_json_path = "./out/Token.sol/Token.json"

[[contracts]]
name = "staking"
contract_json_path = "./out/Staking.sol/Staking.json"
constructor_args = [
    { type = "address", value = "#token" }
]

[[transactions]]
name = "approve-staking"
target = "#token"
function_signature = "approve(address,uint256)"
args = [
    { type = "address", value = "#staking" },
    { type = "uint256", value = "1000000000000000000000" }
]
```

---

## Features

### Automatic NFT Minting

If `hyperaccount-permissioned-minter` is deployed, the tool automatically mints test NFTs:

* Zeroth TBA (Token Bound Account)

* `.os` TBA

### Contract Verification

After deployment, the tool verifies contracts by calling their functions to ensure proper setup.

### Persistent Chain

If Anvil is already running on the specified port, the tool will:

1. Check if contracts are already deployed

2. Skip deployment if found

---

## Troubleshooting

### Port Already in Use

```bash
# Use a different port
kit chain --port 8546
```

### Contract Deployment Failed

* Check that JSON artifacts exist at specified paths (e.g. `./out/.../Contract.json`)

* Run `forge build` again if artifacts are missing

* Verify constructor arguments match contract requirements

* Ensure referenced contracts are defined before use

### Storage Slot Issues

* Storage slots must be 32 bytes (64 hex characters)

* Use `0x` prefix for hex values

* References must point to existing contracts

---

## Integration with Kit

The chain tool integrates with other kit commands:

```bash
# Start chain
kit chain

# In another terminal, boot fake node
kit boot-fake-node --fakechain-port 8545

# Build and deploy package
kit build-start-package
```

---

## Advanced Usage

### Custom RPC Endpoint

```bash
# Connect to external chain (not recommended for development)
kit boot-fake-node --rpc wss://base-mainnet.example.com
```