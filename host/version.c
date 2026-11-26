/* Which YM this is (builder/crates/build-compile/src/generated/version.rs: the release and a fingerprint of the sources), for the log's first line, the
   control bridge, the Discord status and the crash record. builder/crates/build-compile gives YM_VERSION to this file alone: the
   fingerprint changes with every source, and a new one then compiles this one small file, not hos_main.c. */
#ifndef YM_VERSION
#define YM_VERSION "unknown"
#endif

const char *hos_version(void) { return YM_VERSION; }
