#!/bin/bash

# Make sure we are in the right directory
parent_path=$( cd "$(dirname "${BASH_SOURCE[0]}")" ; pwd -P )
cd "$parent_path"

bold () { echo -e "\033[1m$1\033[0m"; }
overwrite() { echo -e "\r\033[1A\033[0K$@"; }

bold "Check if build exists"
if test -f ../target/release/rewatch; 
then
  echo "✅ - Build exists"
else 
  echo "❌ - Build does not exist. Exiting..."
  exit 1
fi

bold "Make sure the testrepo is clean"
if git diff --exit-code ../testrepo &> /dev/null; 
then
  overwrite "✅ - Testrepo has no changes"
else 
  overwrite "❌ - Testrepo is not clean to start with"
  exit 1
fi


bold "Running Tests"
./compile.sh
./watch--change-file.sh
