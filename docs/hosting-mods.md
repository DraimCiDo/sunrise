# Host a modded server

You run a dedicated Sunrise server with `sunrise-server`, without the launcher. This guide distinguishes the
current manual install from the managed setup commands that are coming.

## Install and prepare the game files

Install [Sunrise's server programs](hosting.md#1-install) for your OS. Today, [prepare the game files from your
own Minecraft 26.3 install](hosting.md#2-prepare-the-game-files), or prepare them on another machine and copy them
with `sunrise-server import-resources DIR`. Automatic preparation from Mojang's server JAR is coming; it is not
part of the mod lock check.

## Create the server and install mods

```sh
mkdir smp && cd smp
sunrise-server init --eula
```

`--eula` means you agree to the [Minecraft EULA](https://aka.ms/MinecraftEULA). For a manual content-mod install,
put the `.smod` files and their required dependencies in `sunrise-mods/`. In `sunrise-mods.txt` beside
`server.toml`, enable each one with a `mod.<id>:true` line, for example `mod.coords_hud:true`. Leave `[mods]`
out of `server.toml`: it opts into a managed install and does not enable or download anything by itself.
For native mods today, see [Native server export](hosting.md#native-server-export). Native code runs with the
server's permissions; only install it if you trust its author.

Managed setup with `sunrise-server init --eula --mods ID[,ID…]` or `init --eula --pack ID` is coming. It will
install the selected mods with their dependencies and record what it installed in `mods.lock`.

## Start and inspect the server

```sh
sunrise-server start
sunrise-server status
```

Players join at your server's address (port 25565 by default). To start at boot, use [`sunrise-server service
install`](hosting.md#8-start-it-with-the-computer). See [hosting](hosting.md) for Docker and hosting panels.

In a managed folder, `sunrise-server mods list` shows the versions, on/off choices, native build kinds and reasons
recorded in `mods.lock`. Add `--json` for machine-readable output. This is a local inventory, not a registry
query or a report from the running game. If the [lock, files or engine disagree](hosting.md#managed-mods),
`start` and `run` refuse to launch without changing the installed mods.

## Change mods and share them with players

For a manual install, stop the server before changing its mod files, then start it again. Give joining players
the matching versions of any mods required on both sides.

The managed commands `mods add`, `remove`, `disable`, `enable`, `update`, `search` and `rollback` are coming,
as is updating mods along with the server engine. Until then, the startup checker can identify a drifted lock
but cannot repair it; restore the matching config, engine and files from a known-good copy. `mods client-pack
--out FILE.spack` is also coming. It will export the mods players need, leaving out server-only mods.

## Move the server

Copy the server folder, including the world and its mod files, to the new machine. Install the same Sunrise
engine there and make its matching prepared game files available. The programs and shared game files are not
part of the server folder. For a managed install, copy `mods.lock` with `server.toml` and keep the locked files
at their recorded relative paths.
