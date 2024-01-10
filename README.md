# NecDev

Tools for developing on NectarOS.

## Installing

Install with cargo:

```bash
cargo install --git https://github.com/uqbar-dao/necdev
```

### Updating

To update, re-run

```bash
cargo install --git https://github.com/uqbar-dao/necdev
```

or use
```bash
necdev update
```

## Usage

```bash
# Create a Rust package template (default no UI):
necdev new my_package

# Build the package:
necdev build my_package

# Start a fake node, by default, on port 8080:
necdev boot-fake-node

# Start the package in a running node (requires a node or fake node running at, default, localhost:8080; can specify port of a localhost node with `--port` or can specify entire URL with `--url`):
necdev start-package my_package

# Or build, start a node, and start a package from inside the project...
cd my_package
necdev build
necdev boot-fake-node
necdev start-package

# Bonus: create a Python package template (it `build`s & `start-package`s just like a Rust package!):
necdev new my_py_package -l python
cd my_py_package
necdev build
necdev start-package

# Bonus: create a Rust package template with UI (it `build`s & `start-package`s just like a Rust package!):
necdev new my_package_with_ui --ui
cd my_package_with_ui
necdev build
necdev start-package

# Print usage

necdev --help
```

`necdev boot-fake-node` can also accept a `--runtime-path` argument that compiles the fake node binary from a local Nectar core repository.
Use like (substituting path to Nectar core repo):

```bash
necdev boot-fake-node --runtime-path ~/git/nectar
```

NecDev also contains tools for running tests.
For details and examples, please see [https://github.com/uqbar-dao/core_tests](https://github.com/uqbar-dao/core_tests).

## UI Development

The simplest way to work on the UI is to use `necdev dev-ui` which develops against a running node.
Under the hood, `necdev dev-ui` is just `cd ui && npm install && npm start`.

The UI should open on port `3000` (or next available port) and will proxy all websocket and HTTP requests to `http://localhost:8080` by default.
You can choose to proxy to any URL using the `-u` flag:
```bash
necdev dev-ui my_package -u http://localhost:8081
```
This is the same as prepending the environment variable:
```bash
VITE_NODE_URL=http://localhost:8081 npm start
```

NodeJS (v18 or higher) and NPM are required to build and develop the UI.

The UI is written in React with Vite as the bundler + reloader.
