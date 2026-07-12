<center>
<img src="./assets/comfylang.png" alt="comfy logo">
</center>

**comfylang** is a low-level, statically-typed, Rust/C-flavored systems language, with its compiler (`comfyc`) currently compiling straight to ARM32 assembly.

> **Status: full rewrite in progress.** The compiler is being rebuilt from scratch with a proper architecture (spanned lexer, real diagnostics, a type checker, an IR, and a pluggable backend trait) instead of the original direct AST-walking interpreter-style code generator. The language surface is intentionally minimal right now and will grow step by step. See "Roadmap" below.

## Try it

The dev environment is a [Nix flake](./flake.nix) providing the Rust toolchain, an ARM32 cross toolchain (assembler + linker), and `qemu-arm`, so you can build and run ARM32 binaries from any host architecture:

```sh
nix develop

# compile a .cfy file to ARM32 assembly
cargo run -- examples/exit_code.cfy

# assemble + link the generated assembly
util/link_compile.sh build/main.s

# run the resulting binary (native on ARM32 hosts, qemu-arm everywhere else)
util/host-run.sh build/main

# ...or all of the above in one go:
util/run.sh examples/exit_code.cfy
```

## Current language surface

Right now the compiler understands exactly one thing: a single `fn main()` containing calls to the one compiler intrinsic, `$syscall`:

```rust
// examples/exit_code.cfy
fn main() {
    $syscall(1, 42, 0, 0, 0, 0, 0);
}
```

`$syscall(nr, a0, a1, a2, a3, a4, a5)` maps directly onto the Linux ARM EABI syscall convention (syscall number in `r7`, up to 6 arguments in `r0`-`r5`). Named wrappers like `write`/`read`/`exit` existed in the previous implementation and will come back as ordinary standard-library functions built on top of this single intrinsic, once comfylang has real functions — rather than being hardcoded into the compiler one by one as before.

## Design goals

- Rust-flavored syntax, C-level control: manual memory management, structs, pointers, full type safety.
- Turing-complete, self-hosted: the compiler will eventually be rewritten in comfylang itself.
- Hand-written backend, ARM32 first, with x86 and others planned via a `Backend` trait.
- Compile-time optimizations (constant folding, dead-code elimination, and more) once an IR exists.
- A custom package manager down the line, managing both compiler versions and dependencies.

## Roadmap

Planned, in rough order:

1. ~~Lexer/parser/codegen skeleton with real diagnostics~~ ✅
2. Integer types + variables
3. Expressions (arithmetic, comparison, logical) with precedence
4. Control flow (`if`/`else`, `while`)
5. User-defined functions, calling convention, recursion
6. Pointers, arrays, `struct`s
7. IR + optimization passes
8. Additional backends (x86_64, ...), standard library, self-hosting prep
