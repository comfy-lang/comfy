#include<sys.fy>
#include<"test/local_import.fy">


fn main() {
    $write(1, "hello comfy!\n");

    #if ENABLE_LOGGING
        $write(1, "Logging enabled!\n");
    #else
        $write(1, "Logging disabled!\n");
    #endif

    let hello_text = ":3\n";
    $write(1, hello_text);

    $exit(703);
}