const RETURN_VALUE_BUF: &str = "syscall_ret_val";
const RETURN_VALUE_SIZE: usize = 4;
const RETURN_VALUE_BUF_ALLIGNMENT: usize = 4;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum Register {
    R0,
    R1,
    R2,
    R3,
    R4,
    R5,
    R6,
    R7,
    R8,
    R9,
    R10,
    R11,
    R12,
    SP,
    LR,
    PC,
}

impl Register {
    pub fn as_str(&self) -> &'static str {
        match self {
            Register::R0 => "r0",
            Register::R1 => "r1",
            Register::R2 => "r2",
            Register::R3 => "r3",
            Register::R4 => "r4",
            Register::R5 => "r5",
            Register::R6 => "r6",
            Register::R7 => "r7",
            Register::R8 => "r8",
            Register::R9 => "r9",
            Register::R10 => "r10",
            Register::R11 => "r11",
            Register::R12 => "r12",
            Register::SP => "sp",
            Register::LR => "lr",
            Register::PC => "pc",
        }
    }
}

pub fn syscall_3args(syscall_number: u32, arg0: &str, arg1: &str, arg2: &str) -> String {
    format!(
        "\t{}\n\t{}\n\t{}\n\t{}\n\tsvc #0\n",
        format!("mov r7, #{}", syscall_number),
        into_register_load(arg0, "r0"),
        into_register_load(arg1, "r1"),
        into_register_load(arg2, "r2")
    )
}

pub fn syscall_2args(syscall_number: u32, arg0: &str, arg1: &str) -> String {
    format!(
        "\t{}\n\t{}\n\t{}\n\tsvc #0\n",
        format!("mov r7, #{}", syscall_number),
        into_register_load(arg0, "r0"),
        into_register_load(arg1, "r1")
    )
}

pub fn syscall_1arg(syscall_number: u32, arg0: &str) -> String {
    format!(
        "\t{}\n\t{}\n\tsvc #0\n",
        format!("mov r7, #{}", syscall_number),
        into_register_load(arg0, "r0")
    )
}

fn into_register_load(value: &str, register: &str) -> String {
    if value.chars().all(|c| c.is_ascii_digit()) {
        format!("mov {}, #{}", register, value)
    } else {
        format!("ldr {}, ={}", register, value)
    }
}

#[allow(dead_code)]
pub fn mov_imm(reg: &str, value: usize) -> String {
    format!("\tmov {}, #{}", reg, value)
}

#[allow(dead_code)]
pub fn ldr_label(reg: &str, label: &str) -> String {
    format!("\tldr {}, ={}", reg, label)
}

#[allow(dead_code)]
pub fn store_into_reg(reg: &str, reg_to_store_in: usize) -> String {
    format!("\tstr {}, [{}]\n", reg, reg_to_store_in)
}

#[allow(dead_code)]
pub fn comment(text: &str) -> String {
    format!("\t@ {}", text)
}

#[allow(dead_code)]
pub fn declare_string(label: &str, value: &str) -> String {
    format!("{}: .asciz \"{}\"", label, value)
}

#[allow(dead_code)]
pub fn declare_word(label: &str, value: usize) -> String {
    format!("{}: .word {}", label, value)
}

#[allow(dead_code)]
pub fn declare_lcomm(label: &str, size: usize) -> String {
    format!(".lcomm {}, {}", label, size)
}

#[allow(dead_code)]
pub fn define_len(label: &str) -> String {
    format!("{}_len = .-{}", label, label)
}

#[allow(dead_code)]
pub fn store_syscall_return_value(text: &mut Vec<String>) {
    text.push(format!(
        "\t@ Store syscall return value\n\tldr r4, ={}\n\tstr r0, [r4]\n",
        RETURN_VALUE_BUF
    ));
}

#[allow(dead_code)]
pub fn load_syscall_return_value_into_reg(text: &mut Vec<String>) {
    let reg = Register::R5;
    text.push(format!(
        "\t@ Load syscall return value into {}\n\tldr {}, ={}",
        reg.as_str(),
        reg.as_str(),
        RETURN_VALUE_BUF
    ));

    text.push(format!("\tldr {}, [{}]\n", reg.as_str(), reg.as_str()));
}

#[allow(dead_code)]
pub fn load_syscall_return_value_into_label(text: &mut Vec<String>, label: &str) {
    let ptr_reg: Register = Register::R5;

    text.push(format!(
        "\t@ Load syscall return value into {}\n\
         \tldr {}, ={}\n\
         \tldr r0, [{}]\n",
        label,
        ptr_reg.as_str(),
        RETURN_VALUE_BUF,
        ptr_reg.as_str()
    ));

    text.push(format!(
        "\tldr {}, ={}\n\
         \tstr r0, [{}]\n",
        ptr_reg.as_str(),
        label,
        ptr_reg.as_str()
    ));
}

// ========== UTILITY FUNCTIONS ==========

pub fn generate_assembly(rodata: Vec<String>, bss: Vec<String>, text: Vec<String>) -> String {
    let mut assembly_code = String::new();
    assembly_code.push_str("\n.section .rodata\n");
    for rodata_item in rodata.iter() {
        assembly_code.push_str("\t");
        assembly_code.push_str(rodata_item.as_str());
        assembly_code.push('\n');
    }

    assembly_code.push_str("\n.section .bss\n");
    assembly_code.push_str(&format!(
        "\t.comm {}, {}, {}\n",
        RETURN_VALUE_BUF, RETURN_VALUE_SIZE, RETURN_VALUE_BUF_ALLIGNMENT
    ));
    for bss_item in bss.iter() {
        assembly_code.push_str("\t");
        assembly_code.push_str(bss_item.as_str());
        assembly_code.push('\n');
    }

    assembly_code.push_str("\n.section .text\n");
    for text_item in text.iter() {
        assembly_code.push_str(text_item.as_str());
        assembly_code.push('\n');
    }

    assembly_code
}
