# This file is needed for local development. It makes sure we kill all the
# subprocesses that are hanging around after the main tests get killed, as the
# watcher will need to run in the background.

trap "trap - SIGTERM && kill -- -$$" SIGINT SIGTERM EXIT

#!/bin/bash
parent_path=$( cd "$(dirname "${BASH_SOURCE[0]}")" ; pwd -P )
cd "$parent_path"
./suite-ci.sh
