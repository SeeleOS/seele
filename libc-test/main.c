#include <unistd.h>
#include <errno.h>

// --- 手动定义结构体，绕过 Relibc 头文件的坑 ---

typedef unsigned int  tcflag_t;
typedef unsigned char cc_t;
typedef unsigned int  speed_t;

// Linux x86_64 标准的 termios 结构体布局
struct termios {
    tcflag_t c_iflag;       /* input mode flags */
    tcflag_t c_oflag;       /* output mode flags */
    tcflag_t c_cflag;       /* control mode flags */
    tcflag_t c_lflag;       /* local mode flags */
    cc_t c_line;            /* line discipline */
    cc_t c_cc[32];          /* control characters */
    speed_t c_ispeed;       /* input speed */
    speed_t c_ospeed;       /* output speed */
};

struct winsize {
    unsigned short ws_row;
    unsigned short ws_col;
    unsigned short ws_xpixel;
    unsigned short ws_ypixel;
};

// 声明 ioctl 外部函数，防止 implicit declaration 警告
extern int ioctl(int fd, unsigned long request, ...);

// 简易打印函数
void kprint(const char* s) {
    int len = 0;
    while(s[len]) len++;
    write(1, s, len);
}

int main() {
    struct termios term;
    struct winsize ws;

    kprint("---WHATTHEFUCK Starting BusyBox Readiness Test ---\n");

    // 测试 TCGETS (0x5401)
    if (ioctl(0, 0x5401, &term) != 0) {
        kprint("!!! [ERROR] TCGETS failed.\n");
        return 1;
    }
    kprint("[OK] TCGETS successful.\n");

    // 测试 TIOCGWINSZ (0x5413)
    if (ioctl(0, 0x5413, &ws) != 0) {
        kprint("!!! [ERROR] TIOCGWINSZ failed.\n");
        return 2;
    }
    kprint("[OK] TIOCGWINSZ successful.\n");

    kprint("Please type something and press ENTER: ");

    char c;
    while (read(0, &c, 1) > 0) {
        write(1, &c, 1); // 回显
        if (c == '\n') break;
    }

    kprint("\nBusyBoxTest: OK\n");
    return 0;
}
