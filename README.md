# Rewatch

## [![Release](https://github.com/rolandpeelen/rewatch/actions/workflows/build.yml/badge.svg?branch=master&event=release)](https://github.com/rolandpeelen/rewatch/actions/workflows/build.yml)

# Info

Rewatch is an alternative build system for the [Rescript Compiler](https://rescript-lang.org/) which uses Ninja. It strives to deliver consistent and faster builds in monorepo setups with multiple packages, where the default build system fails to pick up changed interfaces across multiple packages.

# Project Status

This project should be considered Alpha Status. Currently used to solve a very specific problem within [Walnut](https://github.com/teamwalnut/). We're open to PR's and other contributions to make this more solid.

  - [x] Compile Monorepo's with multiple packages
  - [x] Correctly compile to different formats than `.mjs` (taken from bsconfig)
  - [ ] Error Handling - we still panic here-and-there, don't expect a super smooth UX
  - [ ] Compile Single Package
  - [ ] Configure executables - potentially interop with some [Melange](https://github.com/melange-re/melange) / [Bucklescript / ReasonML](https://reasonml.github.io/) subset

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

# Contributing

  Pre-requisites:

  - [Rust](https://rustup.rs/)
  - [NodeJS](https://nodejs.org/en/) - For running testscripts only
  - [Yarn](https://yarnpkg.com/) or [Npm](https://www.npmjs.com/) - Npm probably comes with your node installation

1. `cd testrepo && yarn` (install dependencies for submodule)
  2. `cargo run`

  Running tests:

  1. `./tests/suite.sh`
