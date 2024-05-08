#!/bin/bash

cd $(dirname $0)
source "./utils.sh"
cd "$1" || exit

bold "Test: It should watch"

if rewatch clean &> /dev/null;
then
  success "Repo Cleaned"
else 
  error "Error Cleaning Repo"
  exit 1
fi

exit_watcher() { 
  # we need to kill the parent process (rewatch)
  kill $(pgrep -P $!);
}

rewatch watch &>/dev/null &
success "Watcher Started"

echo 'Js.log("added-by-test")' >> ./packages/main/src/Main.res

sleep 1

if node ./packages/main/src/Main.mjs | grep 'added-by-test' &> /dev/null; 
then
  success "Output is correct"
else 
  error "Output is incorrect"
  exit_watcher
  exit 1
fi

sleep 1

replace '/Js.log("added-by-test")/d' ./packages/main/src/Main.res;

sleep 1

if git diff --exit-code ./  
then
  success "Adding and removing changes nothing"
else 
  error "Adding and removing changes left some artifacts"
  exit_watcher
  exit 1
fi

exit_watcher
