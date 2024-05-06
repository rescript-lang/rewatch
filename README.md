# Rewatch

## [![Release](https://github.com/rolandpeelen/rewatch/actions/workflows/build.yml/badge.svg?branch=master&event=release)](https://github.com/rolandpeelen/rewatch/actions/workflows/build.yml)

# Info

Rewatch is an alternative build system for the [Rescript Compiler](https://rescript-lang.org/) which uses Ninja. It strives to deliver consistent and faster builds in monorepo setups with multiple packages, where the default build system fails to pick up changed interfaces across multiple packages.

# Project Status

This project should be considered in beta status. We run it in production at [Walnut](https://github.com/teamwalnut/). We're open to PR's and other contributions to make it 100% stable in the ReScript toolchain.

# Usage

  1. Install the package

  ```
  yarn add @rolandpeelen/rewatch
  ```

  2. Build / Clean / Watch

  ```
  yarn rewatch build .
  ```

  ```
  yarn rewatch clean .
  ```

  ```
  yarn rewatch watch .
  ```

  Where `.` is the folder where the 'root' `bsconfig.json` lives. If you encounter a 'stale build error', either directly, or after a while, a `clean` may be needed to clean up some old compiler assets.

## Full Options

Find this output by running `yarn rewatch --help`.

```
Usage: rewatch [OPTIONS] [COMMAND] [FOLDER]

Arguments:
  [COMMAND]
          Possible values:
          - build: Build using Rewatch
          - watch: Build, then start a watcher
          - clean: Clean the build artifacts

  [FOLDER]
          The relative path to where the main bsconfig.json resides. IE - the root of your project

Options:
  -f, --filter <FILTER>
          Filter allows for a regex to be supplied which will filter the files to be compiled. For instance, to filter out test files for compilation while doing feature work

  -a, --after-build <AFTER_BUILD>
          This allows one to pass an additional command to the watcher, which allows it to run when finished. For instance, to play a sound when done compiling, or to run a test suite. NOTE - You may need to add '--color=always' to your subcommand in case you want to output colour as well

  -n, --no-timing <NO_TIMING>
          [possible values: true, false]

  -c, --create-sourcedirs <CREATE_SOURCEDIRS>
          This creates a source_dirs.json file at the root of the monorepo, which is needed when you want to use Reanalyze
          
          [possible values: true, false]

      --compiler-args <COMPILER_ARGS>
          

      --rescript-version <RESCRIPT_VERSION>
          

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

# Contributing

  Pre-requisites:

  - [Rust](https://rustup.rs/)
  - [NodeJS](https://nodejs.org/en/) - For running testscripts only
  - [Yarn](https://yarnpkg.com/) or [Npm](https://www.npmjs.com/) - Npm probably comes with your node installation

  1. `cd testrepo && yarn` (install dependencies for submodule)
  2. `cargo run`

  Running tests:

  1. `cargo build --release`
  2. `./tests/suite.sh`
