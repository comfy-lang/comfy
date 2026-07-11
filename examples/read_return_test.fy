fn main() {
    $write(1, "hello comfy!\n");

    buf[64] text;
    let retSize = $read(0, text);
    print_text(text, retSize);

    $exit(0);
}

fn print_text(buf[64] text, size) {
    $write(1, text);
}