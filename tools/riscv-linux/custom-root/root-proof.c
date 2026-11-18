#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static int parse(const char *text, long *value)
{
    char *end;
    errno = 0;
    *value = strtol(text, &end, 10);
    return end != text && *end == '\0' && errno == 0 && *value >= INT32_MIN && *value <= INT32_MAX;
}

int main(int argc, char **argv)
{
    const char *root = getenv("HESPER_ROOT_ID");
    long left, right;
    if (!root || !*root) {
        fputs("HESPER_ROOT_ID is not set\n", stderr);
        return 1;
    }
    if (argc != 3 || !parse(argv[1], &left) || !parse(argv[2], &right)) {
        fputs("usage: root-proof <int32> <int32>\n", stderr);
        return 2;
    }
    printf("ROOT_OK:%s:%ld\n", root, left * right);
    return 0;
}
