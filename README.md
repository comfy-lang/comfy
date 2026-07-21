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

## Examples

**Control flow & recursion:**

```rust
// examples/showcase.cfy
fn factorial(n: int) -> int {
    if n <= 1 {
        return 1;
    }
    return n * factorial(n - 1);
}

fn main() {
    let mut code = 0;
    if factorial(4) >= 20 && !(factorial(4) < 0) {
        code = factorial(4); // 24
    }
    $syscall(1, code, 0, 0, 0, 0, 0); // exit 24
}
```

**Structs, arrays & pointers:**

```rust
// examples/structs/nested.cfy (trimmed)
struct Point {
    x: int,
    y: int,
}

fn main() {
    let mut p = Point { x: 1, y: 2 };
    p.x = p.x + 4; // 5

    let mut nums = [10, 20, 30];
    nums[1] = 99;

    let mut total = 0;
    let mut out = &total;
    *out = p.x + p.y + nums[0] + nums[1] + nums[2]; // 5+2+10+99+30 = 146

    $syscall(1, total, 0, 0, 0, 0, 0); // exit 146
}
```

More examples in [`examples/`](./examples).

## Language reference

**Variables**
- `let NAME = expr;` is an immutable binding. When `expr` is provably constant it's folded away entirely (zero runtime cost); otherwise it falls back to a real, immutable stack slot.
- `let mut NAME = expr;` is always a real stack-allocated local that can be reassigned with `NAME = expr;`.

**Arithmetic & comparisons**
- `+ - * / %` and unary `-` are constant-folded when possible; runtime arithmetic falls back to a simple stack-machine codegen. Runtime (non-constant) `/` and `%` aren't supported yet - arm32 has no hardware divide instruction and the compiler is `-nostdlib`, so it can't call into libgcc for a software fallback.
- `== != < <= > >=` produce a real `bool`, and can't be chained (`a < b < c` doesn't parse).

**Control flow**
- `if`/`else`/`else if`, `while`. Conditions must be `bool` - comfy does not implicitly convert integers to booleans.
- `&& || !` are genuine short-circuiting logical operators.

**Functions**
- Typed parameters (`int`/`bool` so far) and an optional `-> Type` return; omitting it means the function returns `()`.
- Functions must end with a `return` statement if they return a value. Calls follow the AAPCS calling convention (up to 4 arguments in `r0`-`r3`, return value in `r0`), and recursion works.

**Pointers**
- `*T` is a pointer to `T` (recursive, e.g. `**T`).
- `&x` takes the address of a local (only bare identifiers so far); `*p` dereferences, and works both to read (`let y = *p;`) and, as the direct target of `=`, to write (`*p = v;`).

**Arrays**
- `[T; N]` is a fixed-size array of `T`. Array literals (`[e1, e2, ...]`) are the only way to create one, and only as a `let`/`let mut` initializer (not a general expression yet); length and element type are inferred from the literal.
- Indexing (`arr[i]`) works for both reads and, on a `mut` array, writes (`arr[i] = v;`) - the address is computed at runtime (`base + i * elem_size`), with no bounds checking yet. Locals only, not function parameters or return values.

**Structs**
- `struct Name { field: Type, ... }` declares a struct, forward-declared like functions (order-independent).
- Struct literals (`Name { field: value, ... }`) work like array literals - only as a `let`/`let mut` initializer, fields in any order but all required.
- Field access (`s.field`, chainable as `s.a.b.c`) works for reads and, on a `mut` struct, writes, and composes with arrays (`s.arr[i]`).
- A struct can't contain itself by value (infinite size) - only through a pointer (`*Name`), same as recursive data structures in C. Locals only, not function parameters or return values.

**Syscalls**
- `$syscall(nr, a0, a1, a2, a3, a4, a5)` is the sole compiler intrinsic, usable as a statement or an expression (its return value, from `r0`, can be captured). It maps directly onto the Linux ARM EABI syscall convention. Arguments may be pointers as well as integers, for passing buffer addresses. Named wrappers like `write`/`read`/`exit` will come back as ordinary standard-library functions once comfylang has a standard library.

## Optimizations

`comfyc` lowers checked ASTs to a flat, three-address IR before codegen, and runs the following passes on it to a fixpoint, entirely at compile time:

- **Constant propagation** - through virtual registers and stack-local slots alike; a `let` binding that's provably constant is folded away entirely.
- **Redundant load elimination** - a repeated read of an unmodified local becomes a cheap register copy instead of a stack reload.
- **Branch folding** - an `if`/`while` condition that folds to a compile-time constant becomes an unconditional jump (or vanishes) instead of a runtime comparison.
- **Common subexpression elimination** - a repeated computation over unchanged inputs is computed once and reused.
- **Strength reduction** - multiplying by a compile-time power of two becomes a shift; `*0`/`*1` collapse to their trivial equivalents.
- **Dead code elimination** - unused pure computations, unread stores, and unreachable code (after a `return`/unconditional jump) are all removed.
- **Tail-call elimination** - a `return f(...)` in tail position reuses the current stack frame instead of growing the stack, so self-recursive tail calls run in constant space.

Register allocation is a simple linear-scan allocator mapping virtual registers onto a handful of real ARM registers, spilling to the stack only once it runs out.


## Roadmap

Planned, in rough order:

1. ~~Lexer/parser/codegen skeleton with real diagnostics~~ ✅
2. ~~Integer types + variables (`let`/`let mut`, assignment)~~ ✅
3. ~~Arithmetic + comparison expressions with precedence~~ ✅
4. ~~Control flow (`if`/`else`, `while`)~~ ✅
5. ~~Logical operators (`&&`, `||`, `!`) with short-circuit evaluation~~ ✅
6. ~~User-defined functions, calling convention, recursion~~ ✅
7. ~~Pointers (`*T`, `&x`, `*p`)~~ ✅
8. ~~Arrays (`[T; N]`, indexing)~~ ✅
9. ~~`struct`s~~ ✅
10. ~~IR + optimization passes (constant propagation, dead code/branch/store elimination, CSE, strength reduction, tail calls) + linear-scan register allocation~~ ✅
11. Additional backends (x86_64, ...), standard library, self-hosting prep
