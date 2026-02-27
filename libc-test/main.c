#include <stdio.h>
#include <unistd.h>

int main() {
    char buf[64];
    printf("Testing Keyboard. Please type something and press Enter:\n");

    while (1) {
        // 这会触发你的 sys_read
        ssize_t n = read(0, buf, sizeof(buf) - 1);
        if (n > 0) {
            buf[n] = '\0';
            printf("Kernel received: %s", buf);
        }
    }
    return 0;
}
