<center>
<img src="./assets/comfylang.png" alt="comfy logo">
</center>

**comfylang** is a low-level, statically-typed systems language, with the `comfyc` compiler currently compiling straight to ARM32 assembly.

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

`fn main()` plus any number of user-defined functions, with variables, arithmetic, comparisons, `if`/`else`/`while`, and syscalls:

```rust
// examples/showcase.cfy
fn factorial(n: int) -> int {
    if n <= 1 {
        return 1;
    }
    return n * factorial(n - 1);
}

fn sum_up_to(n: int, out: *int) {
    let mut i = 1;
    let mut total = 0;
    while i <= n {
        total = total + i;
        i = i + 1;
    }
    *out = total;
}

fn main() {
    let mut sum = 0;
    sum_up_to(5, &sum); // sum = 1+2+3+4+5 = 15

    let fact = factorial(4); // 24

    let mut code = 0;
    if sum > 10 && fact >= 20 {
        code = sum + fact; // 39
    } else if sum == 0 || !(fact < 0) {
        code = 1;
    }

    $syscall(1, code, 0, 0, 0, 0, 0); // exit 39
}
```

- `let NAME = expr;` is an immutable binding. When `expr` is provably constant it's folded away entirely (zero runtime cost); otherwise it falls back to a real, immutable stack slot. `let mut NAME = expr;` is always a real stack-allocated local that can be reassigned with `NAME = expr;`.
- Arithmetic (`+ - * / %`, unary `-`) is constant-folded when possible; runtime arithmetic falls back to a simple stack-machine codegen. Runtime (non-constant) `/` and `%` aren't supported yet - arm32 has no hardware divide instruction and the compiler is `-nostdlib`, so it can't call into libgcc for a software fallback.
- Comparisons (`== != < <= > >=`) produce a real `bool`, and can't be chained (`a < b < c` doesn't parse). `if`/`while` conditions must be `bool`; comfy does not implicitly convert integers to booleans.
- Functions take typed parameters (`int`/`bool` so far) and an optional `-> Type` return; omitting it means the function returns `()`. Functions must end with a `return` statement if they return a value. Calls follow the AAPCS calling convention (up to 4 arguments in `r0`-`r3`, return value in `r0`) and recursion works.
- `$syscall(nr, a0, a1, a2, a3, a4, a5)` is the sole compiler intrinsic, usable as a statement or an expression (its return value, from `r0`, can be captured). It maps directly onto the Linux ARM EABI syscall convention. Named wrappers like `write`/`read`/`exit` will come back as ordinary standard-library functions built on top of this, once comfylang has a standard library.
- `*T` is a pointer to `T` (recursive, e.g. `**T`). `&x` takes the address of a local (only bare identifiers so far); `*p` dereferences, and works both to read (`let y = *p;`) and, as the direct target of `=`, to write (`*p = v;`). `$syscall` arguments may be pointers as well as integers, for passing buffer addresses.


More examples in [`examples/`](./examples).

## Design goals

- Manual memory management, structs, pointers, full type safety.
- Turing-complete, self-hosted: the compiler will eventually be rewritten in comfylang itself.
- Hand-written backend, ARM32 first, with x86 and others planned via a `Backend` trait.
- Compile-time optimizations (constant folding, dead-code elimination, and more) once an IR exists.
- A custom package manager down the line, managing both compiler versions and dependencies.

## Roadmap

Planned, in rough order:

1. ~~Lexer/parser/codegen skeleton with real diagnostics~~ ✅
2. ~~Integer types + variables (`let`/`let mut`, assignment)~~ ✅
3. ~~Arithmetic + comparison expressions with precedence~~ ✅
4. ~~Control flow (`if`/`else`, `while`)~~ ✅
5. ~~Logical operators (`&&`, `||`, `!`) with short-circuit evaluation~~ ✅
6. ~~User-defined functions, calling convention, recursion~~ ✅
7. ~~Pointers (`*T`, `&x`, `*p`)~~ ✅
8. Arrays (`[T; N]`, indexing)
9. `struct`s
10. IR + optimization passes
11. Additional backends (x86_64, ...), standard library, self-hosting prep
