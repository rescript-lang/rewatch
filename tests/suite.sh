#!/bin/bash

echo "Check if build exists"
test -f target/release/rewatch || echo "Build failed"

# Make sure we are in the right directory
parent_path=$( cd "$(dirname "${BASH_SOURCE[0]}")" ; pwd -P )
cd "$parent_path"

echo "Run tests"
./compile.sh
