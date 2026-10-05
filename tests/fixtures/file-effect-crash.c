/* Linux-only deterministic process-crash fixture. Injects no production hook.
 * The child owns the fixture paths; the parent kills only that stopped child. */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
static int journal_generation;
static int ends(const char *path, const char *suffix) {
    size_t n = strlen(path), m = strlen(suffix);
    return n >= m && strcmp(path + n - m, suffix) == 0;
}
static void checkpoint(const char *phase) {
    const char *selected = getenv("EXITBIND_TEST_CRASH_PHASE");
    const char *marker = getenv("EXITBIND_TEST_CRASH_MARKER");
    if (!selected || !marker || strcmp(selected, phase) != 0) return;
    int fd = open(marker, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0600);
    if (fd < 0) _exit(91);
    if (write(fd, phase, strlen(phase)) < 0) _exit(92);
    close(fd);
    raise(SIGSTOP);
    _exit(93); /* Parent must kill the stopped process, never continue it. */
}
int rename(const char *from, const char *to) {
    int (*real_rename)(const char *, const char *) = dlsym(RTLD_NEXT, "rename");
    int effect = strstr(to, "/.exitbind/effects/") && ends(to, ".json");
    if (effect && journal_generation == 0) checkpoint("before_admission");
    int result = real_rename(from, to);
    if (effect && result == 0) journal_generation++;
    return result;
}
int fsync(int fd) {
    int (*real_fsync)(int) = dlsym(RTLD_NEXT, "fsync");
    int result = real_fsync(fd);
    if (result != 0) return result;
    char link[64], path[4096];
    snprintf(link, sizeof(link), "/proc/self/fd/%d", fd);
    ssize_t n = readlink(link, path, sizeof(path) - 1);
    if (n < 0) return result;
    path[n] = 0;
    if (ends(path, "/.exitbind/effects")) {
        if (journal_generation == 1) checkpoint("after_admission");
        if (journal_generation == 2) checkpoint("after_completion");
    }
    const char *target = getenv("EXITBIND_TEST_TARGET_DIR");
    if (target && strcmp(path, target) == 0 && journal_generation == 1) checkpoint("after_write");
    return result;
}
