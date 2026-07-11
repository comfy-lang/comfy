fn main() {
    $write(1, "hello comfy, inline asm test!\n");

    let test = ":3";
    asm {
        "mov r6, #420" 
    };
    
    $exit(69);
}