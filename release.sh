#!/bin/sh

# Check if we're running in patch mode
if [ "$1" = "patch" ]; then
  # Get the current version from package.json
  current_version=$(jq -r '.version' package.json)
  # Increment the patch version
  version=$(echo $current_version | awk -F. -v OFS=. '{$NF += 1; print}')
  echo "Creating patch release: $current_version -> $version"
elif [ "$1" = "minor" ]; then
  # Get the current version from package.json
  current_version=$(jq -r '.version' package.json)
  # Increment the minor version
  version=$(echo $current_version | awk -F. -v OFS=. '{$2 += 1; $NF = 0; print}')
  echo "Creating minor release: $current_version -> $version"
# ensure that we have at least one argument conforming to semver
elif [ $# -ne 1 ] || ! echo $1 | grep -qE "^[0-9]+\.[0-9]+\.[0-9]+$"; then
  echo "Usage: $0 <version> or $0 patch or $0 minor"
  exit 1
else
  version=$1
fi

# ensure we are on the master branch otherwise exit
if [ $(git rev-parse --abbrev-ref HEAD) != "master" ]; then
  echo "Not on master branch, exiting"
  exit 1
fi

# ensure master is up to date
git pull

# ensure that there are no uncommitted changes
if [ -n "$(git status --porcelain)" ]; then
  echo "There are uncommitted changes, exiting"
  exit 1
fi

# ensure that there are no untracked files
if [ -n "$(git ls-files --others --exclude-standard)" ]; then
  echo "There are untracked files, exiting"
  exit 1
fi

# update the version in package.json
sed -i '' -e "s/\"version\": \".*\"/\"version\": \"$version\"/" package.json

# update the version in cargo.toml
sed -i '' -e "s/^version = \".*\"/version = \"$version\"/" Cargo.toml
cargo build

# commit the changes with the version
git add Cargo.toml package.json Cargo.lock
git commit -m ":rocket: - Release v$version"

# tag current commit with the version
git tag -a v$version -m ":rocket: - Release v$version"

# push the changes
git push origin master

# push the tag
git push origin v$version
