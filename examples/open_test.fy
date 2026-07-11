fn main() {
    $write(1, "hello comfy, open syscall test!\n");

    let fd = $open("/home/asm/comfy", 577, 420);
    $write(fd, "test write for open syscall :3\n");

    $exit(69);
}