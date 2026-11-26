"""Game-free checks of recomplauncher.json, RecompLauncher's profile of YM: it parses, every settings key is a row of
the in-game menu, and its steps are YM-builder's: the release's one program, YM.exe or YM.x86_64, builds the
version folder's game folder builds/default for the profile {config}/YM (build --game ... --profile ...
--output recomplauncher, RecompLauncher's progress and error lines: the profile's profile.json, then the build) and is what
players start (play.rs, with the same --profile: the fast check, the folder updated when needed, then the game), the game
reads the player's .nup where it is, and the saves are the save folders.
That YM-builder takes the step's command line, every tick at once, and plays by the names start runs, is
builder/crates/YM-builder's recomplauncher_s_build_step_is_a_command_line_it_takes.

Run with: python -I tests/test_recomplauncher_json.py
"""
import json
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]


class RecompLauncherJsonTests(unittest.TestCase):
    def test_settings_are_menu_rows(self):
        manifest = json.loads((ROOT / "recomplauncher.json").read_text(encoding="utf-8"))
        self.assertEqual(manifest["format"], 1)
        menu = (ROOT / "gpu" / "vk" / "vk_menu.cpp").read_text(encoding="utf-8")
        rows = {re.sub(" ", "_", name.lower()) for name in re.findall(r'\{TAB_[A-Z]+, "([^"]+)"', menu)}
        # (vk_menu.cpp ini_key: the rows whose key is not their name, renamed rows keeping the key an options file has:
        # output_res, resolution, upscaling, frame_rate_cap, dlss)
        ini_key = menu.split("const char* ini_key(const Row& r) {", 1)[1].split("\n}", 1)[0]
        rows |= set(re.findall(r'return "([a-z_0-9]+)";', ini_key))
        # (#1633: the rows the menu's modules describe, gpu/vk/menu/, by their keys)
        for module in (ROOT / "gpu" / "vk" / "menu").glob("*.cpp"):
            rows |= set(re.findall(r'\.key = "([a-z_0-9]+)"', module.read_text(encoding="utf-8")))
        # (and the game's rows, game/menu/: data the menu adds, by their keys)
        for table in (ROOT / "game" / "menu").glob("*.c"):
            rows |= set(re.findall(r'\.key = "([a-z_0-9]+)"', table.read_text(encoding="utf-8")))
        keys = {key for setting in manifest["settings"] for choice in setting["choices"] for key in choice["set"]}
        self.assertTrue(keys)
        # (keys the game reads that are no menu row: Compile shaders, which the preparation reads, vk_draw.cpp compile_all)
        draw = (ROOT / "gpu" / "vk" / "vk_draw.cpp").read_text(encoding="utf-8")
        read_elsewhere = {"compile_shaders"} if '"compile_shaders="' in draw else set()
        self.assertEqual(keys - rows - read_elsewhere, set())

    def test_builds_into_the_launchers_game_folder(self):
        # (RecompLauncher issue 215: without --out the build went to ~/Games/YM, not the drive the player
        # installed to. The install folder is the version folder (build_driver::play): the program and the game folder
        # builds/default it plays, beside it. The profile is {config}/YM (on Windows %APPDATA%\YM, where the
        # game kept its saves before): the game keeps its saves in its saves/ and its settings in its settings.ini
        # (YM_PROFILE, host/YM_profile.h).)
        manifest = json.loads((ROOT / "recomplauncher.json").read_text(encoding="utf-8"))
        prepare = manifest["prepare"]["args"]
        self.assertEqual(prepare[prepare.index("--profile") + 1], "{config}/YM")
        self.assertNotIn("--out", prepare)
        out, profile = "{install}/builds/default", "{config}/YM"
        play = (ROOT / "builder" / "crates" / "build-driver" / "src" / "play.rs").read_text(encoding="utf-8")
        crate = (ROOT / "builder" / "crates" / "launcher-profile" / "src" / "lib.rs").read_text(encoding="utf-8")
        for const in ('BUILDS: &str = "builds"', 'DEFAULT_BUILD: &str = "default"'):
            self.assertIn(const, play)
        for const in ('SAVES: &str = "saves"', 'SETTINGS: &str = "settings.ini"'):
            self.assertIn(const, crate)
        # (Play starts the release's program for the same profile, which updates that folder and starts the game it
        # holds: play.rs)
        self.assertEqual(manifest["start"]["program"], ["YM.exe", "YM.x86_64"])
        self.assertEqual(manifest["start"]["args"], ["--profile", "{config}/YM"])
        self.assertEqual(manifest["folders"]["game"][0]["path"], out)
        self.assertEqual(manifest["config"][0]["file"][0]["path"], profile + "/settings.ini")
        self.assertIn(profile + "/saves/save", manifest["saves"])
        self.assertNotIn("{home}/Games", json.dumps(manifest))

    def test_saves_are_the_save_folders_on_both_systems(self):
        # (RecompLauncher's backups and cloud saves copy what "saves" names. Started by the launcher's Play, the game keeps
        # its saves in the profile's saves/save (YM_PROFILE), and its mouse's sensitivity in its user folder, the
        # per-user place below, as a game started by itself has it. Started by itself, before the
        # instance, it kept them in <user>/save: on Windows %APPDATA%\YM ({config}/YM, RecompLauncher's AppData\Roaming), where
        # hos_main.c's saves_dir moves <game folder>/user once, or <game folder>/user when that could not move;
        # $XDG_DATA_HOME/YM ({local}/YM) on Linux. That
        # Linux folder, and %LOCALAPPDATA%\YM on Windows, also hold the builder, the launcher, NVIDIA's DLSS files, the
        # NFC tags and the save backups, none of which a save copy may carry: so the save folder, never the whole data folder)
        # (save-work beside it holds a save while it is being written: the game moves it into save at Commit, host/nn_fs.c)
        manifest = json.loads((ROOT / "recomplauncher.json").read_text(encoding="utf-8"))
        profile, linux, roaming = "{config}/YM/saves", "{local}/YM", "{config}/YM"
        self.assertEqual(manifest["saves"], [profile + "/save",
                                             linux + "/save", linux + "/mouse_sens.txt",
                                             roaming + "/save", roaming + "/mouse_sens.txt"])
        self.assertIn('"%s\\\\save\\\\%016llx%016llx"', (ROOT / "host" / "nn_fs.c").read_text(encoding="utf-8"))
        main = (ROOT / "host" / "hos_main.c").read_text(encoding="utf-8")
        self.assertIn('snprintf(beside_user, HOS_PATH_CAP, "%s" SEP_S "user", dir);', main)
        self.assertIn('if (!hos_user_dir) hos_user_dir = saves_dir(beside_user);', main)
        self.assertIn('"/YM"', main)
        self.assertIn('"%s\\\\YM", appdata', main)
        self.assertIn('"%s/mouse_sens.txt", hos_user_dir', (ROOT / "host" / "nn_hid.c").read_text(encoding="utf-8"))

    def test_old_save_copies_go_back_where_they_came_from(self):
        # (RecompLauncher restores a copy made before a move into the new folder at the same place in the list, only when
        # the list keeps its length. The copies made before this list name <user> itself, so a copy of <user>/save/<id>
        # would land in <user>/save/save/<id>. A list of another length restores each copy into the folders it names)
        manifest = json.loads((ROOT / "recomplauncher.json").read_text(encoding="utf-8"))
        before = ["{local}/YM", "{install}/YM/user"]   # (the list before the instance had 6, of another order)
        self.assertNotEqual(len(manifest["saves"]), len(before))

    def test_the_saves_button_opens_the_save_folder_there_is(self):
        manifest = json.loads((ROOT / "recomplauncher.json").read_text(encoding="utf-8"))
        places = manifest["folders"]["saves"]
        self.assertEqual([p.get("if") for p in places[:3]], [p["path"] for p in places[:3]])
        self.assertEqual([p["path"] for p in places],
                         ["{config}/YM/saves/save", "{local}/YM/save", "{config}/YM/save",
                          "{config}/YM/saves"])

    def test_YM_builder_builds_it(self):
        # (the release's program, YM-builder, as recomplauncher.json's zip holds it: its build, with RecompLauncher's
        # progress and error lines on stdout (--output recomplauncher). A build of a folder it built before keeps what did not change, as an update)
        manifest = json.loads((ROOT / "recomplauncher.json").read_text(encoding="utf-8"))
        prepare = manifest["prepare"]
        self.assertEqual(prepare["program"], ["YM.exe", "YM.x86_64"])
        self.assertEqual(prepare["args"][0], "build")
        self.assertEqual(prepare["args"][prepare["args"].index("--profile") + 1], "{config}/YM")
        self.assertEqual(prepare["args"][prepare["args"].index("--output") + 1], "recomplauncher")
        self.assertEqual(prepare["args"][prepare["args"].index("--game") + 1], "{input:game}")
        main = (ROOT / "builder" / "crates" / "YM-builder" / "src" / "main.rs").read_text(encoding="utf-8")
        output = (ROOT / "builder" / "crates" / "build-driver" / "src" / "modhook.rs").read_text(encoding="utf-8")
        self.assertIn('"recomplauncher" => Ok(Output::RecompLauncher)', output)
        self.assertIn("Output::RecompLauncher => Format::Lines", output)
        self.assertIn('"--output" => a.output = Some(output::Output::parse', main)
        report = (ROOT / "builder" / "crates" / "build-core" / "src" / "report.rs").read_text(encoding="utf-8")
        self.assertIn('format!("progress {:.3} {text}"', report)
        self.assertIn('format!("error {}"', report)
        # (nothing of the old builder: its --cli and --prepare, make_game.py's options, the AppImage, its tools)
        text = json.dumps(manifest)
        for old in ("--cli", "--prepare", "--fetch-", "--accept-nvidia", "AppImage", "YMBuilder", "make_game"):
            self.assertNotIn(old, text)
        # (each tick adds an upscaler and the license it is fetched under, YM-builder's --upscaler and --accept-licenses)
        for option in prepare["options"]:
            args = option["args"]
            self.assertEqual(args[0::2], ["--upscaler", "--accept-licenses"], option["id"])
            self.assertIn(args[1], ("dlss", "fsr4", "xess"))
            self.assertIn(args[3], ("nvidia", "amd", "intel"))

    def test_one_program_players_start(self):
        # (RecompLauncher's "Recomp Standards": ship one program and keep it the one players start; it builds the game
        # when it must, then runs it; it is never named builder. Easy Setup off, a shortcut or a Steam entry runs the
        # release's program with start's arguments: by these names and no command, YM-builder plays (play.rs). On Linux
        # not "YM", the game folder's name beside it)
        manifest = json.loads((ROOT / "recomplauncher.json").read_text(encoding="utf-8"))
        start, prepare = manifest["start"]["program"], manifest["prepare"]["program"]
        self.assertEqual(start, prepare)
        for name in start:
            self.assertNotIn("/", name)
            self.assertNotIn("builder", name.lower())
            self.assertNotEqual(name, manifest["folders"]["game"][0]["path"].rsplit("/", 1)[1])
        play = (ROOT / "builder" / "crates" / "YM-builder" / "src" / "play.rs").read_text(encoding="utf-8")
        self.assertIn('pub const NAMES: [&str; 2] = ["%s", "%s"];' % tuple(start), play)
        # (its exit statuses, explained: never built, a build, update or start that failed, with its log, and the
        # profile in use by another start, single.rs)
        codes = manifest["exit_codes"]
        self.assertIn("pub const NOT_BUILT: i32 = 120;", play)
        self.assertIn("pub const FAILED: i32 = 121;", play)
        single = (ROOT / "builder" / "crates" / "YM-builder" / "src" / "single.rs").read_text(encoding="utf-8")
        self.assertIn("pub const IN_USE: i32 = 123;", single)
        self.assertEqual(set(codes), {"120", "121", "123"})
        self.assertIn("already running", codes["123"])
        self.assertIn('pub const LOG: &str = "logs/YM_update.log";', play)
        self.assertIn("builds/default/logs/YM_update.log", codes["121"])
        self.assertEqual(manifest["folders"]["logs"], [{"path": "{install}/builds/default/logs"}])

    def test_the_game_reads_the_copy_where_it_is(self):
        # (YM-builder keeps no copy of the game files in the folder: the game reads the .nup in place, so the step
        # needs the player's copy each time, and the copy must stay where it is. No keeps_copy)
        manifest = json.loads((ROOT / "recomplauncher.json").read_text(encoding="utf-8"))
        self.assertFalse(manifest["prepare"].get("keeps_copy", False))
        self.assertIn("keep it there", manifest["inputs"][0]["label"])

    def test_the_work_folder_is_the_builder_s_own(self):
        # (no --work: YM-builder's per-user cache, ~/.cache/YM-builder or %LOCALAPPDATA%\YM\Builder
        # (YM_CACHE), with the stores every game folder on the PC shares, so a second build compiles only what changed)
        manifest = json.loads((ROOT / "recomplauncher.json").read_text(encoding="utf-8"))
        self.assertNotIn("--work", manifest["prepare"]["args"])
        self.assertNotIn("--store", manifest["prepare"]["args"])

    def test_a_finished_build_is_told_by_its_record(self):
        # (prepare.done: YM_build.json, the record a build writes when it finishes, what update builds from)
        manifest = json.loads((ROOT / "recomplauncher.json").read_text(encoding="utf-8"))
        out = manifest["folders"]["game"][0]["path"]
        self.assertEqual(manifest["prepare"]["done"], [out + "/YM_build.json"])
        folder = (ROOT / "builder" / "crates" / "build-driver" / "src" / "folder.rs").read_text(encoding="utf-8")
        self.assertIn('pub const RECORD_FILE: &str = "YM_build.json";', folder)

    def test_the_copy_is_the_input_game(self):
        # (RecompLauncher's named input game: the .nup or the extracted folder, refused when its build ID isn't the one
        # YM is made from)
        manifest = json.loads((ROOT / "recomplauncher.json").read_text(encoding="utf-8"))
        self.assertNotIn("game", manifest)
        copy = manifest["inputs"][0]
        self.assertEqual((copy["id"], copy["role"]), ("game", "game"))
        self.assertEqual([f["type"] for f in copy["formats"]], ["file", "folder"])
        self.assertEqual(copy["check"], {"nup_build_id": "893c30e7"})

    def test_a_release_asks_for_a_new_build_by_itself(self):
        # (RecompLauncher builds the game again whenever the release it installed changes: no prepare.version to stamp, and
        # no timeout, which it no longer reads)
        prepare = json.loads((ROOT / "recomplauncher.json").read_text(encoding="utf-8"))["prepare"]
        self.assertNotIn("version", prepare)
        self.assertNotIn("timeout", prepare)

    def test_repository_is_this_forks_github_release(self):
        # (RecompLauncher's "repo" is a GitHub owner/name: this fork's releases are GitHub releases of YMRecomp)
        manifest = json.loads((ROOT / "recomplauncher.json").read_text(encoding="utf-8"))
        self.assertEqual(manifest["repo"], "hashil-3503/YMRecomp")


if __name__ == "__main__":
    unittest.main()
