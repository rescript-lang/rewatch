# Rewatch

[![Release](https://github.com/rolandpeelen/rewatch/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/rolandpeelen/rewatch/actions/workflows/build.yml)
[![Tests](https://github.com/rolandpeelen/rewatch/actions/workflows/test.yml/badge.svg?branch=master)](https://github.com/rolandpeelen/rewatch/actions/workflows/test.yml)

------------------------
# Info

Rewatch is an alternative build system for the [Rescript Compiler](https://rescript-lang.org/) which uses Ninja. It strives to deliver consistent and faster builds in monorepo setups with multiple packages.


# Project Status
- [ ] Compile Monorepo's with multiple packages
- [ ] Compile Single Package
- [ ] Configure executables - potentially interop with some [Melange](https://github.com/melange-re/melange) / [Bucklescript / ReasonML](https://reasonml.github.io/) subset


# Contributing

Pre-requisites:
- [Rust](https://rustup.rs/) 
- [NodeJS](https://nodejs.org/en/) - For running testscripts only
- [Yarn](https://yarnpkg.com/) or [Npm](https://www.npmjs.com/) - Npm probably comes with your node installation

1. `cd testrepo && yarn` (install dependencies for submodule)
2. `cargo run`

Running tests:
1. `./tests/suite.sh`
