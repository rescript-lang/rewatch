#!/bin/bash
# Make sure we are in the right directory
cd $(dirname $0)

source ./utils.sh

bold "Check if build exists"
if test -f ../target/release/rewatch; 
then
  success "Build exists"
else 
  error "Build does not exist. Exiting..."
  exit 1
fi

bold "Make sure the testrepo_yarn is clean"
if git diff --exit-code ./testrepo_yarn &> /dev/null; 
then
  success "testrepo_yarn has no changes"
else 
  error "testrepo_yarn is not clean to start with"
  exit 1
fi

bold "Make sure the testrepo_pnpm is clean"
if git diff --exit-code ./testrepo_pnpm &> /dev/null; 
then
  success "testrepo_pnpm has no changes"
else 
  error "testrepo_pnpm is not clean to start with"
  exit 1
fi

bold "Yarn Tests"
./compile.sh "testrepo_yarn" \
        && ./watch.sh "testrepo_yarn" \
        && ./lock.sh "testrepo_yarn" \
        && ./suffix.sh "testrepo_yarn";

bold "PNPM Tests"
./compile.sh "testrepo_pnpm" \
        && ./watch.sh "testrepo_pnpm" \
        && ./lock.sh "testrepo_pnpm" \
        && ./suffix.sh "testrepo_pnpm";
