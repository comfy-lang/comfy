fn main() {
    let mut i = 0;
    let mut sum = 0;

    while i < 10 {
        sum = sum + i;
        i = i + 1;
    }

    if sum == 45 {
        $syscall(1, 1, 0, 0, 0, 0, 0); // correct: 0+1+...+9 = 45
    } else {
        $syscall(1, 0, 0, 0, 0, 0, 0);
    }
}
