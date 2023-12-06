# UqDev

Tools for developing on Uqbar

## Installing

Clone this repo, and then install with cargo:

```
git clone https://github.com/uqbar-dao/uqdev
cd uqdev
cargo install --path .
```

## Usage

```
# Create a new project package template:
uqdev new my_package -p my_package

# Build the package:
uqdev build my_package

# Start the package in a running node
uqdev start-package my_package -u http://localhost:8080

# Or from inside the project...
cd my_package
uqdev build
uqdev start-package -u http://localhost:8080

# Run tests (see https://github.com/uqbar-dao/core_tests for more details):
uqdev run-tests

# Print usage

uqdev --help
uqdev build --help
uqdev inject-message --help
uqdev start-package --help
uqdev run-tests --help
```

## TODO

1. Update README Installing section when repo goes public: `cargo install --git https://github.com/uqbar-dao/uqdev`
2. Put crate on crates.io
