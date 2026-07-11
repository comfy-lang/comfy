pub struct SectionWriter {
    pub rodata: Vec<String>,
    pub bss: Vec<String>,
    pub text: Vec<String>,
}

impl SectionWriter {
    pub fn new() -> Self {
        let mut text = Vec::new();
        text.push(".global _start".to_string());

        Self {
            rodata: Vec::new(),
            bss: Vec::new(),
            text,
        }
    }

    // ====== TEXT SECTION HELPERS ======

    pub fn push_text(&mut self, line: impl Into<String>) {
        self.text.push(line.into());
    }

    #[allow(dead_code)]
    pub fn push_text_lines(&mut self, lines: &[impl ToString]) {
        self.text.extend(lines.iter().map(|l| l.to_string()));
    }

    // ====== BSS SECTION HELPERS ======

    pub fn declare_bss(&mut self, label: &str, size: usize) {
        self.bss.push(format!(".lcomm {}, {}", label, size));
    }

    #[allow(dead_code)]
    pub fn declare_bss_with_name_prefix(&mut self, prefix: &str, name: &str, size: i32) {
        self.bss
            .push(format!(".lcomm {}_{}, {}", prefix, name, size));
    }

    #[allow(dead_code)]
    pub fn declare_bss_with_len(&mut self, label: &str, size: i32) {
        self.bss.push(format!(".lcomm {}, {}", label, size));
        self.bss.push(format!("{}_len = {}", label, size));
    }

    // ====== RODATA SECTION HELPERS ======

    #[allow(dead_code)]
    pub fn push_rodata_str(&mut self, label: &str, value: &str) {
        self.rodata.push(format!("{}: .asciz \"{}\"", label, value));
    }

    pub fn push_rodata_word(&mut self, label: &str, value: i32) {
        self.rodata.push(format!("{}: .word {}", label, value));
    }

    pub fn push_rodata_str_with_len(&mut self, label: &str, value: &str) {
        self.rodata.push(format!("{}: .asciz \"{}\"", label, value));
        self.rodata.push(format!("{}_len = .-{}", label, label));
    }

    #[allow(dead_code)]
    pub fn all_sections(&self) -> String {
        let mut out = String::new();

        if !self.rodata.is_empty() {
            out += ".section .rodata\n";
            out += &self.rodata.join("\n");
            out += "\n\n";
        }

        if !self.bss.is_empty() {
            out += ".section .bss\n";
            out += &self.bss.join("\n");
            out += "\n\n";
        }

        if !self.text.is_empty() {
            out += ".section .text\n";
            out += &self.text.join("\n");
            out += "\n";
        }

        out
    }
}
