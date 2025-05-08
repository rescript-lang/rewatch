#!/bin/bash

rm -rf testrepo/node_modules
cp -rf . ../rescript/rewatch
cd testrepo
yarn install
cd ..
rm -rf ../rescript/rewatch/.git
rm -rf ../rescript/rewatch/docs
rm -rf ../rescript/rewatch/.github
cd ../rescript/rewatch
rm .tmuxinator.yaml
rm package.json
rm release.sh
rm sync_upstream.sh
# reset yarn.lock to the most recent commited version
git checkout -- testrepo/yarn.lock
cd testrepo
yarn install
