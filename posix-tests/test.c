#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <termios.h>
#include <unistd.h>
#include <dirent.h>

static int test_hello(int argc, char **argv) {
    (void)argc;
    (void)argv;
    puts("hello: ok");
    return 0;
}

static int test_getcwd(int argc, char **argv) {
    (void)argc;
    (void)argv;
    char buf[512];
    if (!getcwd(buf, sizeof(buf))) {
        perror("getcwd");
        return 1;
    }
    printf("cwd: %s\n", buf);
    return 0;
}

static int test_chdir(int argc, char **argv) {
    const char *path = (argc > 2) ? argv[2] : "/";
    if (chdir(path) != 0) {
        perror("chdir");
        return 1;
    }
    return test_getcwd(argc, argv);
}

static int test_open_read(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr, "open_read: need path\n");
        return 1;
    }
    int fd = open(argv[2], O_RDONLY);
    if (fd < 0) {
        perror("open");
        return 1;
    }
    char buf[128];
    ssize_t n = read(fd, buf, sizeof(buf));
    if (n < 0) {
        perror("read");
        close(fd);
        return 1;
    }
    printf("read %ld bytes\n", (long)n);
    if (n > 0) {
        write(STDOUT_FILENO, buf, (size_t)n);
        write(STDOUT_FILENO, "\n", 1);
    }
    close(fd);
    return 0;
}

static int test_stat(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr, "stat: need path\n");
        return 1;
    }
    struct stat st;
    if (stat(argv[2], &st) != 0) {
        perror("stat");
        return 1;
    }
    printf("mode: %o size: %ld\n", (unsigned)st.st_mode, (long)st.st_size);
    return 0;
}

static int test_readdir(int argc, char **argv) {
    const char *path = (argc > 2) ? argv[2] : "/";
    DIR *d = opendir(path);
    if (!d) {
        perror("opendir");
        return 1;
    }
    struct dirent *ent;
    while ((ent = readdir(d)) != NULL) {
        printf("%s\n", ent->d_name);
    }
    closedir(d);
    return 0;
}

static int test_fork_wait(int argc, char **argv) {
    (void)argc;
    (void)argv;
    pid_t pid = fork();
    if (pid < 0) {
        perror("fork");
        return 1;
    }
    if (pid == 0) {
        puts("child: exit 42");
        _exit(42);
    }
    int status = 0;
    pid_t got = waitpid(pid, &status, 0);
    if (got < 0) {
        perror("waitpid");
        return 1;
    }
    if (WIFEXITED(status)) {
        printf("parent: child exit %d\n", WEXITSTATUS(status));
    } else {
        printf("parent: child status %d\n", status);
    }
    return 0;
}

static int test_exec(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr, "exec: need path\n");
        return 1;
    }
    pid_t pid = fork();
    if (pid < 0) {
        perror("fork");
        return 1;
    }
    if (pid == 0) {
        char *child_argv[] = { argv[2], NULL };
        execve(argv[2], child_argv, NULL);
        perror("execve");
        _exit(127);
    }
    int status = 0;
    waitpid(pid, &status, 0);
    return 0;
}

static int test_termios(int argc, char **argv) {
    (void)argc;
    (void)argv;
    struct termios t;
    if (tcgetattr(STDIN_FILENO, &t) != 0) {
        perror("tcgetattr");
        return 1;
    }
    printf("iflag=%u oflag=%u cflag=%u lflag=%u\n",
           (unsigned)t.c_iflag,
           (unsigned)t.c_oflag,
           (unsigned)t.c_cflag,
           (unsigned)t.c_lflag);
    return 0;
}

static int test_write(int argc, char **argv) {
    const char *msg = (argc > 2) ? argv[2] : "write: ok\n";
    ssize_t n = write(STDOUT_FILENO, msg, strlen(msg));
    return (n < 0) ? 1 : 0;
}

struct test_case {
    const char *name;
    int (*fn)(int, char **);
};

static struct test_case TESTS[] = {
    {"hello", test_hello},
    {"getcwd", test_getcwd},
    {"chdir", test_chdir},
    {"open_read", test_open_read},
    {"stat", test_stat},
    {"readdir", test_readdir},
    {"fork_wait", test_fork_wait},
    {"exec", test_exec},
    {"termios", test_termios},
    {"write", test_write},
};

int main(int argc, char **argv) {
    if (argc < 2) {
        fprintf(stderr, "usage: %s <test> [args...]\n", argv[0]);
        fprintf(stderr, "tests:");
        for (size_t i = 0; i < sizeof(TESTS) / sizeof(TESTS[0]); ++i) {
            fprintf(stderr, " %s", TESTS[i].name);
        }
        fprintf(stderr, "\n");
        return 1;
    }
    for (size_t i = 0; i < sizeof(TESTS) / sizeof(TESTS[0]); ++i) {
        if (strcmp(argv[1], TESTS[i].name) == 0) {
            return TESTS[i].fn(argc, argv);
        }
    }
    fprintf(stderr, "unknown test: %s\n", argv[1]);
    return 1;
}
