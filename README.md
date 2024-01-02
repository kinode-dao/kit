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

### Updating

To update, re-run

```bash
cargo install --git https://github.com/uqbar-dao/uqdev
```

or use
```bash
uqdev update
```

## Usage

```bash
# Create a Rust package template (default no UI):
uqdev new my_package

# Build the package:
uqdev build my_package

# Start a fake node, by default, on port 8080:
uqdev boot-fake-node

# Start the package in a running node (requires a node or fake node running at, default, localhost:8080; can specify port of a localhost node with `--port` or can specify entire URL with `--url`):
uqdev start-package my_package

# Or build, start a node, and start a package from inside the project...
cd my_package
uqdev build
uqdev boot-fake-node
uqdev start-package

# Bonus: create a Python package template (it `build`s & `start-package`s just like a Rust package!):
uqdev new my_py_package -l python
cd my_py_package
uqdev build
uqdev start-package

# Bonus: create a Rust package template with UI (it `build`s & `start-package`s just like a Rust package!):
uqdev new my_package_with_ui --ui
cd my_package_with_ui
uqdev build
uqdev start-package

# Print usage

uqdev --help
```

`uqdev boot-fake-node` can also accept a `--runtime-path` argument that compiles the fake node binary from a local Uqbar core repository.
Use like (substituting path to Uqbar core repo):

```bash
uqdev boot-fake-node --runtime-path ~/git/uqbar-v2/uqbar
```

UqDev also contains tools for running tests.
For details and examples, please see https://github.com/uqbar-dao/core_tests

## UI Development

The simplest way to work on the UI is to use `uqdev dev-ui` which develops against a running node.
Under the hood, `uqdev dev-ui` is just `cd ui && npm install && npm start`.

The UI should open on port `3000` (or next available port) and will proxy all websocket and HTTP requests to `http://localhost:8080` by default.
You can choose to proxy to any URL using the `-u` flag:
```bash
uqdev dev-ui my_package -u http://localhost:8081
```
This is the same as prepending the environment variable:
```bash
VITE_NODE_URL=http://localhost:8081 npm start
```

NodeJS (v18 or higher) and NPM are required to build and develop the UI.

The UI is written in React with Vite as the bundler + reloader.
