# Contributing to YM

YM is a personal fork of [Longshot](https://git.magicbean.dev/MagicBeanDev/Longshot), retargeted to Yoshi and the
Mysterious Book. It is not run by the Longshot team, and the Longshot team's channels, forge and review rules do not
apply here: please don't take questions or reports about YM to them. Issues and pull requests for YM go to this
repository on GitHub.

Much of [the developer docs](docs/dev/reference/) still describes Longshot's team and its process (its forge, chat,
CI and review rules); read them for how the code works, not for how to contribute here.

## The rule with no exceptions

**No game files, keys, or anything made from them leave your PC.** The repository does not contain the game and never
will. That includes:

- the `.nup`, `romfs`, `exefs`, `.nso`, `.nca`, `.nsp`, `.xci` or `.nsz`/`.xcz` files, and any title keys;
- the builder's work folders and stores (`~/.cache/YM-builder`, `%LOCALAPPDATA%\YM\Builder`, or the folder `--cache`
  names: the game's code modules, `image.bin`, the lifted IR in `gen/`, the objects), `YM_romfs.idx` and `work/`;
- `YM.exe`, `YM_vk.dll` and other build output, run profiles (perf or Tracy captures), frame dumps, shader or pipeline
  dumps, and the game folder's `cache/`;
- logs and crash dumps from a game run, screenshots of debugging output, and input recordings (they hold the game's
  memory);
- saves, `YM_options.ini`, `YM_env.txt` and `language.txt`.

Don't ask in an issue or a pull request where to get the game, how to decrypt it, or for someone else's files.
`.gitignore` covers most of this, but check `git status` before you commit, and never `git add -f` around it. If you
aren't sure whether a file is game data, leave it out.

## Licensing

YM is free software under the [GNU General Public License, version 3 or later](LICENSE). By submitting a change you
license it under those terms, so only send code you can license that way: nothing from a project with an incompatible
license, and no decompiled game code or anything generated from the game.

How the game or the console behaves (a function's name and arguments, a result code, the order of calls) is a fact and
not code, and the host's replacements for the console's functions are written from such facts; describe them in your
own words and cite a public source when there is one.

The third-party parts (Eden, suyu, Sirit, the Vulkan Memory Allocator, SDL, Opus, Dear ImGui, zstd, the Libertinus
fonts, zbic, AMD's FidelityFX headers and SDK, the CRT shaders, Kenney's input prompts and Tracy, among others; and
a64-kit, whose notice for host/ and builder/rt is host/NOTICE.TOTKPort.md) keep their own notices: preserve them, and
don't relicense their code. `third_party/` holds unmodified upstream copies (`scripts/check_third_party.py`).

Comments marked `(longshot)` are Longshot's changes and additions, `(YM)` this fork's. Keep the existing marks as they
are when you edit nearby.

NVIDIA's DLSS SDK, AMD's FSR runtime and Intel's `libxess.dll` are never committed, bundled or uploaded: only XeSS's
MIT API headers are in the repository (`third_party/xess`).

## Building and testing

The game-free checks run with `python -I -B tests/run_all.py`. On Windows, put the bundled compiler on `PATH` first
(`tools/llvm-mingw/bin`, installed from `tools.lock` by `mise run tools`). Build and run the game only from your own
copy, and keep everything it makes on your PC.
