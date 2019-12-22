#!/bin/bash

set -e

LD_LIBRARY_PATH=/opt/intel/libsgx-enclave-common/aesm /opt/intel/libsgx-enclave-common/aesm/aesm_service

dirpath=$(cd $(dirname $0) && pwd)
cd "${dirpath}/../core"
echo $PWD
export SGX_MODE=HW
export ANONIFY_URL=172.18.0.3:8080
export ETH_URL=172.18.0.2:8545

echo "Start building core components."

if [ "x$1" == "x--release" ]; then
    make
    rm -rf ../example/bin && cp -rf bin/ ../example/bin/ && cd ../example/server

    echo "Build artifacts in release mode, with optimizations."
    cargo run --release
    exit
fi

make DEBUG=1
rm -rf ../example/bin && cp -rf bin/ ../example/bin/ && cd ../example/server

echo "Build artifacts in debug mode."
RUST_LOG=debug cargo run