# Contributing to comfylang

Thanks for your interest in comfylang! It's an early-stage, low-level, statically-typed
systems language with an ARM32-targeting compiler (`comfyc`). Because the project is
young, the roadmap in [`README.md`](./README.md#roadmap) is the source of truth for
what's planned - check there before starting large work.

## Getting started

The dev environment is a [Nix flake](./flake.nix) providing the Rust toolchain, an
ARM32 cross toolchain, and `qemu-arm`:

```sh
nix develop

# compile a .cfy file to ARM32 assembly
cargo run -- examples/exit_code.cfy

# assemble + link the generated assembly
util/link_compile.sh build/main.s

# run the resulting binary (native on ARM32, qemu-arm elsewhere)
util/host-run.sh build/main

# ...or all of the above in one go
util/run.sh examples/exit_code.cfy
```

If you're not using Nix, you need a stable Rust toolchain (2024 edition) plus an
ARM32 (`arm-none-eabi` or similar) assembler/linker and `qemu-arm` on your `PATH`.

## Finding something to work on

- Look for issues labeled `good first issue` or `help wanted`.
- Check the [roadmap](./README.md#roadmap) for the next unchecked item - these are
  usually the highest-value contributions.
- For anything touching language syntax or semantics, please open a
  **Language design discussion** issue first, so the approach can be agreed on before
  you invest time in an implementation.

## Making changes

1. Fork and branch from `main`.
2. Keep changes focused; unrelated cleanup should be a separate PR.
3. If you add or change language surface (new syntax, new semantics), add a short
   example under [`examples/`](./examples) and update the "Current language surface"
   section of `README.md`.
4. Make sure `cargo build` succeeds before opening a PR.
5. Fill out the PR template, including which `area:` the change belongs to.

## Reporting bugs

Please include a minimal `.cfy` snippet that reproduces the problem, the command you
ran, and what you expected vs. what happened. The bug report issue template will walk
you through this.

## Proposing language features

Open a **Feature request / language proposal** issue. Include the motivation
(what can't you express today?) and, if possible, a sketch of the syntax you have in
mind. Larger or more contentious proposals may get moved to a **Language design
discussion** issue first.
