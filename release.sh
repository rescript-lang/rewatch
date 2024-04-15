#!/bin/sh

# ensure we are on the master branch otherwise exit
if [ $(git rev-parse --abbrev-ref HEAD) != "master" ]; then
  echo "Not on master branch, exiting"
  exit 1
fi

# update the version in package.json
npm version $1

# update the version in cargo.toml
sed -i '' -e "s/^version = \".*\"/version = \"$1\"/" Cargo.toml

# commit the changes with the version
git add Cargo.toml package.json
git commit -m "Release $1"

# tag current commit with the first argument
git tag -a $1 -m "Release $1"

# push the changes
git push origin master

# push the tag
git push origin $1
