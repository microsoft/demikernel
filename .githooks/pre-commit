#!/bin/sh

verify_if_rustfmt_is_installed() {
  if ! command -v rustfmt > /dev/null 2>&1; then
    echo "Error: rustfmt is not installed. Please install rustfmt."
    exit 1
  fi
}

check_code_formatting() {
  directory=$1
  current_directory=$(pwd)
  cd $directory
  if ! cargo fmt -- --check; then
    echo "Error: Rust code is not formatted. Please run 'cargo fmt' in the $directory directory to format your code automatically."
    exit 1
  fi
  cd $current_directory
}

verify_if_all_crates_are_formatted() {
  check_code_formatting .
  check_code_formatting ./dpdk_bindings
  check_code_formatting ./network_simulator
  check_code_formatting ./xdp_bindings
}

verify_if_rustfmt_is_installed
verify_if_all_crates_are_formatted
exit 0

