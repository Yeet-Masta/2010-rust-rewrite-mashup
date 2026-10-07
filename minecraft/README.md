# Minecraft

Just Minecraft: a real, endless Minecraft 26.3 world in a window, played as
vanilla survival or creative. No MW2, no Skate 3, no game files of your own.

It is built from the same code as the mashup's Minecraft map:

- [`third_party/minecraftoss`](../third_party/minecraftoss/README.md): the
  MinecraftOSS engine crates: world generation, the chunk map, the player
  (vanilla movement, survival, inventory, crafting, loot) and the mobs.
- [`crates/minecraft_terrain`](../crates/minecraft_terrain): terrain
  streaming, lighting, section meshing, the sky, and the mob, item and
  particle renderers.

This project adds what the mashup took from IW4L: a window, a wgpu renderer
for MinecraftOSS's shaders, vanilla's HUD and screens drawn from Minecraft's
own GUI textures, sounds, input, and saving.

## What's in it

- Minecraft's own world generation from a seed, with its day and night, sky,
  clouds, lighting and fog. Water, lava and fire are animated.
- Vanilla movement: walking, sprinting, sneaking, jumping, swimming, and
  flying in creative.
- Survival: health, hunger, fall damage, eating, death and respawning.
  Experience from orbs.
- Mining at vanilla speeds with the right tools, block drops from
  Minecraft's loot tables, tool wear.
- Placing blocks the way vanilla places them: torches on walls, doors, beds,
  stairs, slabs, logs, chests and so on. Crops need farmland, plants need
  soil. Hoes till, shovels make paths, axes strip logs. Doors, trapdoors,
  levers and buttons work.
- The inventory with its 2x2 crafting grid, the crafting table, and a
  creative item list. Every vanilla recipe.
- Passive and hostile mobs with their AI, which spawn, wander, attack and
  drop loot. Spawn eggs.
- Minecraft's block, mob, footstep and item sounds.
- Worlds save to disk, with every edit and mob.

## Running it

```bash
cargo run --release
```

or run the built `minecraft` (`minecraft.exe` on Windows).

The first time, it downloads Minecraft 26.3's own files (textures, sounds,
world data) from Mojang's official servers, as the launcher does: about
125 MB, into `minecraft-data/minecraft-26.3`. That needs an internet
connection and `curl`, which comes with Windows 10 and 11, macOS and most
Linux distributions. After that it plays offline. Nothing from Minecraft is
included in this repository.

`minecraft-data` is the folder of that name where you run the game, if
there is one, and otherwise the one next to the program. It also holds the
saves, screenshots, `logs/latest.log` (and the session before it,
`logs/previous.log`), and a report for each crash in `crash-reports`.

| Option | |
| --- | --- |
| `--world NAME` | The world to play, in `minecraft-data/saves/NAME` (default `world`) |
| `--seed SEED` | The seed of a new world: a number or any text |
| `--creative` | Creative mode |
| `--view-distance N` | Chunks to render around you (default 8) |
| `--time TICKS` | Time of day to start at: 1000 morning, 6000 noon, 13000 dusk, 18000 midnight |
| `--temporary` | A fresh world that isn't saved |
| `--no-vsync` | Don't wait for the display between frames (up to 120 a second), as vanilla's VSync setting turned off |
| `--data DIR` | Where Minecraft's files and the saves live (default `minecraft-data`, as above) |

A saved world keeps its seed and game mode; `--creative` turns a survival
world creative. The game saves when you quit through the menu, close the
window or close its console, and every five minutes.

## Controls

| Keys | Action |
| --- | --- |
| Mouse, WASD | Look and move |
| Space | Jump; twice quickly to fly in creative |
| Left Shift | Sneak; fly down |
| Left Ctrl, or W twice | Sprint |
| Left click | Mine, attack |
| Right click | Place, use, eat |
| Middle click | Pick the block you're looking at |
| 1-9, mouse wheel | Select a hotbar slot |
| E | Inventory (the item list in creative) |
| Q | Drop the selected item; Ctrl+Q the whole stack |
| F1 | Hide the HUD |
| F2 | Screenshot, into `minecraft-data/screenshots` |
| F3 | Debug info |
| F11 | Fullscreen |
| Esc | Menu |

In the inventory: shift-click moves a stack across, right click takes half
a stack or places one, a double click gathers matching items, dragging a stack shares
it out, and 1-9 over a slot swaps it with the hotbar.

## Building

It's its own Cargo workspace; the mashup's workspace leaves it out.

```bash
cd minecraft
cargo build --release
```

Linux needs the usual window and sound libraries (Debian and Ubuntu:
`libasound2-dev libx11-dev libxkbcommon-x11-0 pkg-config`) and a Vulkan or
OpenGL driver.

The Windows build cross-compiles from Linux as the mashup's does, with
[`cargo-xwin`](https://github.com/rust-cross/cargo-xwin) and LLVM's
`clang-cl`, `lld-link` and `llvm-lib`:

```bash
rustup target add x86_64-pc-windows-msvc
cargo install cargo-xwin --locked
cargo xwin build --release --target x86_64-pc-windows-msvc
```

## Credits

- [MinecraftOSS](../third_party/minecraftoss/README.md): the Rust Minecraft
  engine behind the world, the player and the mobs.
- Minecraft is a trademark of Mojang Studios, and its files are downloaded
  from Mojang, not redistributed here. This project isn't affiliated with
  Mojang or Microsoft.
