use std::collections::HashMap;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum Architecture {
    Arm32,
    Arm64,
    X86,
    X86_64,
}

fn arm32_syscalls() -> HashMap<&'static str, u32> {
    HashMap::from([
        ("exit", 1),
        ("read", 3),
        ("write", 4),
        ("open", 5),
        ("sysinfo", 143),
    ])
}

pub fn get_syscall_num(arch: Architecture, name: &str) -> Option<u32> {
    match arch {
        Architecture::Arm32 => arm32_syscalls().get(name).cloned(),
        Architecture::Arm64 => panic!("ARM64 syscalls not implemented yet"),
        Architecture::X86 => panic!("X86 syscalls not implemented yet"),
        Architecture::X86_64 => panic!("X86_64 syscalls not implemented yet"),
    }
}

pub fn get_syscall_num_or_panic(arch: Architecture, name: &str) -> u32 {
    get_syscall_num(arch, name).unwrap_or_else(|| panic!("Unknown syscall `{}` for {:?}", name, arch))
}