<center>
<img  src="./assets/comfylang.png"  alt="comfy logo">
</center>  

**comfy** is a low-level, compiled scripting language with direct [arm32](https://en.wikipedia.org/wiki/ARM_architecture_family#32-bit_architecture) syscall access.

## Features

- Direct access to arm32 syscalls
- Simple syntax for type-safe, low-level programming
- Compiles to arm32 assembly

## Example  
The following example writes `hello comfy!` to stdout and exits with exit code `0`.
```
fn main() {
  str hello_text = "hello comfy!\n";
  $write(1, hello_text);
  $exit(0);
}
```

## Currently supported syscalls
**comfy** provides direct wrappers to arm32 syscalls, stripping away
boilerplate code such as setting up registers manually.
Syscall wrappers are prefixed with `$`.
The following syscalls are currently supported or next in development:

| Supported? | Syscall # | Syscall Name | Wrapper Function | Description | Return Value |
| --- | --- | --- | --- | --- | --- |
| ✅ | 1 | `exit` | `$exit(status)` | Terminate the calling process. | Does not return. |
| ✅ | 3 | `read` | `$read(fd, buf)` | Read from a file descriptor. | Number of bytes read, or -1 on error.|
| ✅ | 4 | `write` | `$write(fd, buf)` | Write to a file descriptor. | Number of bytes written, or -1 on error.|
| ✅  | 5 | `open` | `$open(path, flags)` | Open a file. | File descriptor, or -1 on error. |
| ✅  | 143 | `sysinfo` | `$sysinfo(buf)` | Get system information. | 0 on success, -1 on error. |

  
  
  

## Variables
**comfy** uses explicit types, which have to be stated when a variable is initialized. Currently available types are `bool`, `char`, `int8`, `int16`, `int32` and `str`.  

### Example  
```comfy
fn main() {
  str text = "hello!\n";
  $write(1, text);

  int8 code = 0;
  $exit(code);
}
```

Variables are immutable by default. To declare a variable as mutable, use the `mut` keyword.
  
```comfy
fn main() {
  mut int8 exit_code = 1;

  exit_code = 0;

  $exit(exit_code);
}

```

## Include System & Conditional Compilation

**comfy** supports a simple preprocessor system similar to C/C++, allowing you to include external files and conditionally compile blocks of code using configuration flags.

### File Inclusion

You can include system or user files using the `#include` directive.

* `#include<sys>` – loads a system library from the comfy standard path (e.g., `/usr/include/comfylang/sys`)
* `#include<"./custom.fy">` – loads a user-defined file relative to your project or include path

These inclusions are handled before compilation and act as if the contents of the file were directly pasted at the `#include` line.

#### Example

```comfy
#include<sys>
#include<"lib/utils.fy">

fn main() {
  $write(1, helloText);
  $exit(0);
}
```

---

### Conditional Compilation

comfy supports `#if`, `#else`, and `#endif` to include or exclude code depending on flags set in your `project.comfx` configuration file.

These flags are defined as key-value pairs under the `[defines]` section of the config, and evaluated before compilation.

#### Supported conditions:

* `#if FLAG_NAME`
* `#if FLAG_NAME == "value"`
* `#if FLAG_NAME != "value"`
* `#else`
* `#endif`

#### Example

```comfy
#if ENABLE_LOGGING
  $write(1, "Logging is on\n");
#endif

#if VERSION == "1.2.3"
  $write(1, "Version matches\n");
#else
  $write(1, "Version mismatch\n");
#endif
```

#### Example config (`project.comfx` in TOML format)

```toml
[defines]
ENABLE_LOGGING = "true"
VERSION = "1.2.3"
```

---

## Roadmap
Project progress, planned and future features can be viewed on the [Project board](https://github.com/users/crnvl/projects/8).
