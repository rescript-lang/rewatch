# Development

1. Install [Rust](https://rustup.rs/)
2. Install [Cargo Watch](https://crates.io/crates/cargo-watch)
3. `git submodule init --update` (initialize the repo in `/walnut_monorepo`)
4. `cd walnut_monorepo && yarn` (install dependencies for submodule)
5. `cargo watch -x run`

# Additional Info

- Build m1 specific release: `RUSTFLAGS="-C target-cpu=apple-m1" cargo build --release`
- Build docs: `cargo doc --no-deps --document-private-items --target-dir ./docs`

# Compilation Process

1. Create the same folder hierarchy in a new build directory:
   .build/...

2. Compile all .res / .resi files to an AST

```bash
bsc.exe  -bs-v 10.1.0,1671455790.,1665412235.,1671455790. -ppx '/Users/jfrolich/development/walnut_monorepo/node_modules/@jfrolich/bisect_ppx/ppx --exclude-files .*\.cy\.res$$' -ppx /Users/jfrolich/development/walnut_monorepo/node_modules/decco/ppx -ppx '/Users/jfrolich/development/walnut_monorepo/node_modules/@reasonml-community/graphql-ppx/ppx -schema=../../api/schema.graphql' -bs-jsx 3 -open TeamwalnutStdlib.Stdlib -absname -bs-ast -o $out $i
```

Nothing out of the ordinary here, just passing in the file, output file, and things like the ppx's that need to be passed trough, also an argument to automatically open certain modules.

Because you can specify the $out file, you can pass the path of the build directory

I think we can use timestamps to see if we need to regenerate the AST file

3. Compile dependencies of each file

If there is an AST we need to get the dependencies of each file. This is written in a file in the original build system, but we can just as well store this info in memory.

```bash
bsb_helper.exe -hash d1837ac4bea4be797c5959c6eb537787 -bs-ns TeamwalnutApp $in
```

This command gets the deps of each file. It passes in a namespace of the current package (configurable in bsconfig).

No idea what the hash is

The deps are given in stdout

4. Compile the AST file to to a .cmi .cmj and .mjs file

If all dependencies are compiled, the rescript compiler can generate the mjs file. It also generates the .cmi (binary interface typed tree) and .cmj (binary typed tree) files. These are used by the compiler if this file is a dependency. For each dependency these file should be generated, and the directory of these files should be included.

```bash
bsc.exe -bs-ns TeamwalnutApp -I . -I src/insights/funnelAnalysis -I src/insights/dashboard/demo/visitors  -I /Users/jfrolich/development/walnut_monorepo/node_modules/@teamwalnut/bs-popper/lib/ocaml -I /Users/jfrolich/development/walnut_monorepo/node_modules/@teamwalnut/bindings/lib/ocaml -open TeamwalnutStdlib.Stdlib  -bs-package-name @teamwalnut/app -bs-package-output es6:$in_d:.mjs -bs-v $g_finger $i
```

This command is run from the ./lib/bs directory (i removed the absolute path of the executable).

No idea what the $in_d, finding this in the source code, perhaps the path of the file? (input directory?)

```c
  if (var == "in_d") {
    return MakePath(*(edge_->inputs_.begin()), REMOVE_BASENAME);
  }
```

What is interesting is that for packages all compiler assets are "installed" in a flat directory called /lib/ocaml while for the current project the compiler assets are referenced in a directory structure (relative to /lib/bs). Even though the compiler assets of packages are in a flat directly they are still resolved correctly like "@teamwalnut/models/src/Models.mjs". No idea what magic is causing this. But I wonder if compiler assets can just be directly generated in a flat directory structure. It might be faster (and simpler!) and module names are unique anyway. I suspect the $in_d is the original path of this file and that it's embedded in the typed tree, so all references to this file use the correct import statement.

No idea what the g_finger is here??

for the rest it's pretty standard.
