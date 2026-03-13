#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <termios.h>
#include <unistd.h>

extern char **environ;

static int has_slash(const char *s) {
    for (; *s; ++s) {
        if (*s == '/') {
            return 1;
        }
    }
    return 0;
}

static const char *resolve_self_path(const char *argv0) {
    static char buf[256];
    if (argv0 && has_slash(argv0)) {
        return argv0;
    }
    if (!argv0) {
        argv0 = "bash-test.elf";
    }
    snprintf(buf, sizeof(buf), "/programs/%s", argv0);
    return buf;
}

static int test_getcwd_chdir(void) {
    char buf[256];
    char old[256];
    if (!getcwd(old, sizeof(old))) {
        perror("getcwd");
        return 1;
    }
    if (chdir("/") != 0) {
        perror("chdir /");
        return 1;
    }
    if (!getcwd(buf, sizeof(buf))) {
        perror("getcwd after chdir");
        return 1;
    }
    if (buf[0] != '/') {
        fprintf(stderr, "getcwd returned non-absolute path: %s\n", buf);
        return 1;
    }
    if (chdir(old) != 0) {
        perror("chdir back");
        return 1;
    }
    return 0;
}

static int test_termios(void) {
    struct termios t;
    if (tcgetattr(0, &t) != 0) {
        perror("tcgetattr");
        return 1;
    }
    if (tcsetattr(0, TCSANOW, &t) != 0) {
        perror("tcsetattr");
        return 1;
    }
    return 0;
}

static int test_stat_open(const char *self_path) {
    struct stat st;
    if (stat(self_path, &st) != 0) {
        perror("stat self");
        return 1;
    }
    int fd = open(self_path, O_RDONLY);
    if (fd < 0) {
        perror("open self");
        return 1;
    }
    char buf[8];
    ssize_t n = read(fd, buf, sizeof(buf));
    if (n <= 0) {
        perror("read self");
        close(fd);
        return 1;
    }
    close(fd);
    return 0;
}

static int test_fork_exec_pipe(const char *self_path) {
    int fds[2];
    if (pipe(fds) != 0) {
        perror("pipe");
        return 1;
    }
    pid_t pid = fork();
    if (pid < 0) {
        perror("fork");
        return 1;
    }
    if (pid == 0) {
        close(fds[0]);
        if (dup2(fds[1], 1) < 0) {
            _exit(120);
        }
        close(fds[1]);
        char *child_argv[] = {(char *)self_path, "__child", NULL};
        execve(self_path, child_argv, environ);
        _exit(127);
    }

    close(fds[1]);
    char out[32];
    ssize_t n = read(fds[0], out, sizeof(out) - 1);
    close(fds[0]);
    if (n <= 0) {
        perror("read pipe");
        return 1;
    }
    out[n] = '\0';
    int status = 0;
    if (waitpid(pid, &status, 0) < 0) {
        perror("waitpid");
        return 1;
    }
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        fprintf(stderr, "child failed: status=%d\n", status);
        return 1;
    }
    if (strstr(out, "child-ok") == NULL) {
        fprintf(stderr, "pipe output mismatch: %s\n", out);
        return 1;
    }
    return 0;
}

int main(int argc, char **argv) {
    if (argc > 1 && strcmp(argv[1], "__child") == 0) {
        const char *msg = "child-ok";
        write(1, msg, strlen(msg));
        return 0;
    }

    const char *self_path = resolve_self_path(argc > 0 ? argv[0] : NULL);

    if (test_getcwd_chdir() != 0) {
        fprintf(stderr, "FAIL: getcwd/chdir\n");
        return 1;
    }
    if (test_termios() != 0) {
        fprintf(stderr, "FAIL: termios\n");
        return 1;
    }
    if (test_stat_open(self_path) != 0) {
        fprintf(stderr, "FAIL: stat/open\n");
        return 1;
    }
    if (test_fork_exec_pipe(self_path) != 0) {
        fprintf(stderr, "FAIL: fork/exec/pipe/dup2\n");
        return 1;
    }

    printf("Successful\n");
    return 0;
}
