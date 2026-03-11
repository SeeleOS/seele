# 任务清单（高 → 低）

## 能跑就行

1. **execve argv/envp 通路打通**  
   内核 `Execve` 接收 argv/envp，用户态构造正确的参数/环境。
2. **close/fd 生命周期语义完善**  
   对齐现有对象表，`fork/exec` 继承行为稳定。
3. **open flags 最小集落地**  
   `O_RDONLY/O_WRONLY/O_RDWR/O_CREAT/O_TRUNC/O_APPEND`。
4. **stat/fstat/lstat 兼容**  
   `FileInfo` 覆盖 relibc 常用路径。
5. **getdents/posix_getdents 支持**  
   让 `readdir/ls` 能工作。
6. **PATH 搜索/execvp 兼容**  
   `busybox/sh` 能直接运行 applet。
7. **termios 基本 ioctl 覆盖**  
   `TCGETS/TCSETS/TIOCGWINSZ` 已有则验证。
8. **waitpid 返回值/状态可靠**  
   `EAGAIN` 轮询 + 正确 status 写入。
9. **TTY 读写稳定**  
   `stdin/out/err` 交互可用。
10. **最小 sh**  
    分词、`cd/exit`、`fork+execve+waitpid`。

## 优化

1. **pipe 实现**  
   管道/重定向/组合命令。
2. **dup/dup2/dup3**  
   fd 重定向/管道必需。
3. **信号最小集**  
   `SIGCHLD/SIGINT/sigaction/kill`。
4. **job control**  
   前台/后台、`TIOCSPGRP/TIOCGPGRP`。
5. **非阻塞 I/O + poll/select**  
   更复杂工具所需。
6. **mmap/brk 完整语义**  
   更大程序/动态分配。
7. **线程/TLS 稳定性**  
   更复杂 libc 组件依赖。
