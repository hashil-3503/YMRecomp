/* (longshot) What the game does when it is started by itself in a version folder (docs/dev/reference/build.md, "The game
   started by itself"):
   - The hand-over: the game started by itself in a version folder's game folder (<version>/builds/<build>/, the version
     folder holding the builder) hands over to the builder, `<builder> play -- <its arguments>`, so the folder is checked
     and the game started with the default profile. The builder gives a game it starts its channel,
     YM_BUILDER_CHANNEL (host/builder_channel.h): a game with one never hands over. Anywhere else (a developer's
     folder, dev.py's, the Test Farm's), and for a --prepare-shaders run, the game runs as it always did.
   (How the game asks the builder for a build, restart_to_apply, is over that channel: build.md, "The game's channel to
   its builder".)
   Plain C, no system call but the one given: tests/test_YM_exit.py builds it alone. */
#pragma once

#include <stdlib.h>
#include <string.h>
#include <wchar.h>
#include "builder_channel.h"
#include "YM_profile.h"

/* (YM_PROFILE, the profile the builder gives the game, and its paths: host/YM_profile.h) */
#define YM_BUILDS "builds"
#ifdef _WIN32
#define YM_BUILDER_NAME "YM.exe"
#else
#define YM_BUILDER_NAME "YM.x86_64"
#endif

static inline int YM_is_sep(char c) { return c == '/' || c == '\\'; }

/* The length of path's folder (path up to its last separator, that one left out): 0 when it has none */
static inline size_t YM_dir_len(const char *path, size_t n) {
    while (n && !YM_is_sep(path[n - 1])) n--;
    return n ? n - 1 : 0;
}

/* Whether the game should hand over to its version folder's builder. game_path: the game's program (hos_game_path_a's:
   the game folder's, a bin/ one left out); channel: getenv(YM_BUILDER_CHANNEL); is_file: whether a path is a file;
   builder_name: YM_BUILDER_NAME. On 1, builder holds the builder's program */
static inline int YM_handover(const char *game_path, const char *channel, int argc, char **argv,
                                    int (*is_file)(const char *), const char *builder_name, char *builder, size_t bcap) {
    if (channel && channel[0]) return 0;
    for (int i = 1; i < argc; i++)
        if (!strcmp(argv[i], "--prepare-shaders")) return 0;   /* (the build's own run of the game) */
    if (!game_path) return 0;
    const size_t folder = YM_dir_len(game_path, strlen(game_path));   /* <version>/builds/<build> */
    const size_t builds = YM_dir_len(game_path, folder);               /* <version>/builds */
    const size_t root = YM_dir_len(game_path, builds);                 /* <version> */
    const size_t name = builds - (root + 1);
    if (!folder || !builds || !root || name != strlen(YM_BUILDS) ||
        strncmp(game_path + root + 1, YM_BUILDS, name))
        return 0;
    if (root + 2 + strlen(builder_name) >= bcap) return 0;
    memcpy(builder, game_path, root);
    builder[root] = game_path[root];
    strcpy(builder + root + 1, builder_name);
    return is_file(builder);
}

/* The builder's command line for the hand-over, as an argv (malloc, NULL-ended): builder play -- argv[1..]. NULL out
   of memory */
static inline char **YM_handover_argv(const char *builder, int argc, char **argv) {
    char **v = (char **)malloc(sizeof *v * (size_t)(argc + 3));
    if (!v) return NULL;
    int k = 0;
    v[k++] = (char *)builder, v[k++] = (char *)"play", v[k++] = (char *)"--";
    for (int i = 1; i < argc; i++) v[k++] = argv[i];
    v[k] = NULL;
    return v;
}

/* Windows: a command line's arguments after its program's name (as Windows splits it: a quoted name to its closing
   quote, else to the first space or tab), the spaces before them left out */
static inline const wchar_t *YM_cmdline_tail(const wchar_t *line) {
    if (*line == L'"') {
        line++;
        while (*line && *line != L'"') line++;
        if (*line) line++;
    } else
        while (*line && *line != L' ' && *line != L'\t') line++;
    while (*line == L' ' || *line == L'\t') line++;
    return line;
}
