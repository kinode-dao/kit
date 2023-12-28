# UqDev

Tools for developing on Uqbar

## Installing

Install with cargo:

```bash
# Get utility to build Python:
pip3 install componentize-py==0.7.1

# Get nvm, node, npm for building front-ends:
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.7/install.sh | bash

# Then, in a new terminal:
nvm install node
nvm install-latest-npm

# Install `uqdev` tools:
cargo install --git https://github.com/uqbar-dao/uqdev
```

## Usage

```bash
# Create a new project package template:
uqdev new my_package

# Build the package ("--ui" is optional):
uqdev build my_package --ui

# Start a fake node, by default, on port 8080:
uqdev boot-fake-node

# Start the package in a running node (requires a node or fake node running at port given in --url):
uqdev start-package my_package --url http://localhost:8080

# Or build, start a node, and start a package from inside the project...
cd my_package
uqdev build
uqdev boot-fake-node
uqdev start-package -u http://localhost:8080

# Print usage

uqdev --help
uqdev new --help
uqdev build --help
uqdev inject-message --help
uqdev boot-fake-node --help
uqdev start-package --help
uqdev run-tests --help
```

`uqdev boot-fake-node` can also accept a `--runtime-path` argument that compiles the fake node binary from a local Uqbar core repository.
Use like (substituting path to Uqbar core repo):

```bash
uqdev boot-fake-node --runtime-path ~/git/uqbar-v2/uqbar
```

UqDev also contains tools for running tests.
For details and examples, please see https://github.com/uqbar-dao/core_tests

## UI Development

NodeJS (v18 or higher) and NPM are required to build and develop the UI.

The UI is written in React with Vite as the bundler + reloader.

To develop locally against a node running on port 8080, run `npm install` and `npm start`. The UI should open on port `3000` and will proxy all websocket and HTTP requests to the local node.

If the node is running on a different port or at a remote URL, the proxy target can be changed on line 19 of `ui/vite.config.ts` or VITE_API_URL="*target*" can be added before the `npm start` command like `VITE_API_URL="*target*" npm start`.
