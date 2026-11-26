<h1 align="center">YM</h1>

<h4 align="center">An unofficial static recompilation of Yoshi and the Mysterious Book for Windows & Linux, without emulation.</h4>

YM translates the game's code to LLVM IR and compiles it into a native Windows or Linux program, with the graphics
rebuilt on Vulkan and the console's system software reimplemented. All of this happens **on your PC, from your own
copy of the game**.

> [!IMPORTANT]
> This repository **does not contain the game, keys or an executable**, and it does not explain how to get them. You
> need your own copy (build `893c30e7`). Everything the build creates (`YM.exe`, the generated code, the shader caches
> and your saves) comes from your game and stays on your PC. Never share those files.

## Status

Work in progress, on Windows. The game boots to its title screen: the title video (H.264) plays, the audio comes out
(`nn::audio`'s AudioOut), and it holds about 60 fps. Gameplay, controllers, saving and runs longer than a minute are
not checked yet. There is no release or launcher: you build it yourself.

## Where it comes from

YM is a personal fork of [Longshot](https://git.magicbean.dev/MagicBeanDev/Longshot) (itself a continuation of
[Hookshot](https://github.com/NotRexed/Hookshot)), retargeted from Ocarina of Time to Yoshi and the Mysterious Book
and renamed. It was forked from Longshot's `main` of 2026-11-25 and modified since; it is not run by the Longshot team.
Comments marked `(longshot)` are Longshot's changes, `(YM)` this fork's.

Some code under `game/` and `assets/` is still Longshot's Ocarina of Time support (its patches, saves, pipeline lists
and profiles); the build reads some of it, and it is being taken apart.

## Building

1. **Tools**: `mise run tools` installs the compiler and libraries `tools.lock` pins into `tools/` (Windows:
   `tools/llvm-mingw`, `tools/sdl`, `tools/cpm`). Each is SHA-256 checked; they come from Longshot's store, with a
   mirror on this repository's [build-tools-1 release](https://github.com/hashil-3503/YMRecomp/releases/tag/build-tools-1).
2. **The builder**: `cargo build --release -p YM-builder`.
3. **The game**, from your own `.nup`:

   ```powershell
   $W = "<a work folder outside the repository>"
   .\target\release\YM-builder.exe image --game "<your .nup>" --work $W --out "<the game folder>"
   .\target\release\YM-builder.exe lift --work $W --root .
   .\target\release\YM-builder.exe compile --work $W --root . --output text   # always with --root .
   ```

   then copy `$W\build\YM.exe` and `YM_vk.dll` into the game folder. Keep the work folder and the game folder out of
   the repository.

The game-free checks: `python -I -B tests/run_all.py` (on Windows with `tools/llvm-mingw/bin` on `PATH`). How the
builder, the hooks, the switches and the GPU replay work is in [docs/dev/reference](docs/dev/reference/) (written for
Longshot; the code is the same). [CONTRIBUTING.md](CONTRIBUTING.md) has the rules: above all, no game files.

YM sends nothing anywhere: its crash, performance and build reports have no server, and Discord's status stays off
until you give it an application id of your own (`discord_app_id=`).

## License

YM is free software under the [GNU General Public License, version 3 or later](LICENSE) (GPL-3.0-or-later).
It is a modified version of Longshot, a modified version of Hookshot, which NotRexed released under the same license;
the changes made since each fork are their authors'.

    Hookshot: Copyright (C) 2026 NotRexed and the Hookshot contributors
    Longshot: Copyright (C) 2026 the Longshot contributors
    YM: Copyright (C) 2026 the YM contributors

It includes code from Eden and suyu, which are under the GPL too. Third-party parts under other licenses that
work with the GPL (Sirit, the Vulkan Memory Allocator, SDL, Opus, Dear ImGui, zstd, the Libertinus fonts and zbic) keep
their own notices in their folders; TOTKPort's notice for the parts after a64-kit (host/, builder/rt) is
host/NOTICE.TOTKPort.md.

YM is an unofficial fan project, not affiliated with or endorsed by Nintendo. Yoshi, The Legend of Zelda and Ocarina
of Time are trademarks of Nintendo.

## Credits

YM builds on Longshot, and through it on Hookshot. These are Longshot's credits, kept as Longshot wrote them ("Longshot" there is Longshot, not this fork):

Longshot is a fork of NotRexed's [Hookshot](https://github.com/NotRexed/Hookshot), and exists thanks to the work of many
people:

- **NotRexed**: [Hookshot](https://github.com/NotRexed/Hookshot), which Longshot is a fork of, BlueShell (Mario Kart World),
  Skybound (Tears of the Kingdom) and the a64-kit tools (host/ and the lifter's runtime are after them).
- **[Eden](https://git.eden-emu.dev/eden-emu/eden)** and **Eglantine**: the shader recompiler and Switch 2 shader work.
- **suyu**: the graphics decoders and the AArch64 translator.
- **[seedds/nxnative](https://github.com/seedds/nxnative)**: reference for how task and mesh shaders work.
- **[EvilBoyce / evilmfkr](https://github.com/evilmfkr/Hookshot)** (EvilOOT): the GPU culling that brings back Kokiri's foliage
  (trees, ferns, grass, falling leaves, fireflies), the ambient light probes, the pond's water, the game's own fog, the freeze
  after Load (Horizon's mutex handoff) and the exposure flicker fix.
- **[zortos293/Lichii](https://github.com/zortos293/Lichii)**: reference for Ampere shader lowering, and its compare-exchange retry fix.
- **[mstorsjo/llvm-mingw](https://github.com/mstorsjo/llvm-mingw)**: the bundled Windows compiler (no Visual Studio needed).
- **NVIDIA DLSS**: builds made with NVIDIA's DLSS SDK (optional, downloaded by each build, never bundled) use NVIDIA DLSS
  Super Resolution, NVIDIA's technology. NVIDIA and DLSS are trademarks of NVIDIA Corporation.
- **AMD FSR 3.1 and FSR 4**: every build has AMD FidelityFX Super Resolution 3.1, built from AMD's MIT-licensed FidelityFX
  SDK shaders; builds made with FSR 4 (optional; AMD's DLLs are never bundled) use AMD FidelityFX Super Resolution 4
  through AMD's MIT-licensed FSR API headers. Both are AMD's technology. AMD, Radeon and FidelityFX are trademarks of Advanced Micro
  Devices, Inc.
- **[kinnay/zbic](https://github.com/kinnay/zbic)**, **Opus** and **SDL**: decompression, audio and controllers.
- **[sostenesfreitas/ocarina-recomp](https://github.com/sostenesfreitas/ocarina-recomp)**: this README's layout, and its notes
  confirmed the mesh stage's control section.
- **[MythosOfs/Orchid](https://github.com/MythosOfs/Orchid)**: the fog and exposure workaround the game used before
  its fog and lighting were fixed.
- **Mesa (NAK)**: the reference for Ampere's instruction encoding.
- Everyone who sent fixes: SkippySteve (Linux and the AppImage, and many more), monsterlag, klimovich008, hckrman101 (NVIDIA
  DLSS, DLAA and Reflex, the exposure flicker and fog fixes, build speed, CI), madebyjordan (AMD FSR 4.1), N3oTraX (FSR 4 on
  Linux), shayderrr, gab9281, Wpaif, nicholasgower, ZurielAtr, Joey-Magnus, Zorkats,
  d3zessete, chrispository, vzeol, leonidasmark, Segmentree, ShyVortex, hiKareeem and i900gaming, among others.
