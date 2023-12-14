# UqDev

Tools for developing on Uqbar

## Installing

Install with cargo:

```bash
cargo install --git https://github.com/uqbar-dao/uqdev
```

## Usage

```bash
# Create a new project package template:
uqdev new my_package -p my_package

# Build the package:
uqdev build my_package

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
