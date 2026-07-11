
fn main() {
    $write(1, "hello comfy!\n");

    buf[64] text;
    $read(0, text);
    $write(1, text);

    $exit(69);
}