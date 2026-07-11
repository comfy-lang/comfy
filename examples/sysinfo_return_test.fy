fn main() {
    $write(1, "Testing sysinfo return value\n");

    buf[88] sysinfo_buf;
    let result = $sysinfo(sysinfo_buf);

    $write(1, "Sysinfo call completed with return value stored in result\n");
    $exit(0);
}