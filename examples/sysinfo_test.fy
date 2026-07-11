fn main() {
    $write(1, "Testing sysinfo syscall\n");

    buf[88] sysinfo_buf;
    let result = $sysinfo(sysinfo_buf);

    $write(1, "Sysinfo call completed\n");
    $exit(0);
}