# flipwire - Cross-Platform Bluetooth Flipper Control

![demo GIF of Flipwire](docs/demo.gif)

Flipwire lets you control your Flipper Zero from any computer over
Bluetooth just like the mobile app. Flipwire is currently only a
command-line tool.

# Usage
Flipwire will attempt to connect to your Flipper before any
operation. Pair your Flipper to your computer before using Flipwire so
that they can find each other.

Run `flipwire` with no arguments to see the built-in help. Command
line usage is basically:

```
$ flipwire [-d] -f <Flipper name> <command> <arguments...>
```

Flags:

- `-d`: disconnect from Flipper on exit (optional)
- `-f <Flipper name>`: Flipper name, like `Uwuw2` if your Flipper is named `Uwuw2` (required)

Commands:

- `upload <src> <dest>`: upload a file to a path on the Flipper. The
  destination path must include the filename. For example: `upload
  picopass.fap /ext/apps/NFC/picopass.fap`.
- `download <src> <dest>`: download a file from the Flipper to a local
  file.
- `ul <src> <dest>`: upload a file to the Flipper (just like `upload`)
  and attempt to launch it as an app. The file must be a `.fap`.
- `launch <app>`: launch a `.fap` file on the Flipper.
- `ls <dir>`: list a directory on the Flipper.
- `alert`: play an alert on the Flipper to help you find it.
- `synctime`: sync the Flipper's clock to the computer's clock.

## Flipper paths
The Flipper uses a Unix-style path system to specify paths in internal
and external storage. Most likely you want to interact with external
storage (the SD card), which the Flipper sees at `/ext`. `/ext` is
also the directory you browse when you use the Browser tool on the
Flipper itself.

Example paths:

- `/ext/apps/NFC/nfc.fap`: the FAP (external application) file for the NFC app
- `/ext/infrared`: the directory where IR files are saved

In an MSYS shell on Windows, you have to suppress path translation for
Flipwire to work, otherwise it will receive Flipper paths as local
system paths. In Git Bash, set `MSYS_NO_PATHCONV=1`. In MSYS2, set
`MSYS2_ARG_CONV_EXCL="*"`. See [this StackOverflow
answer](https://stackoverflow.com/a/34386471).

# FAQ
## Why "flipwire"?
It sounds cool. Plus, "flip" is sort of a synonym of "invert", and the
opposite of a wire is wireless, and Flipwire lets you do things with
your Flipper wirelessly...you get the idea.

## Why Rust?
Because I like Rust. Also, because the Flipper ecosystem has a strong
"plug and play" mentality, and Rust makes it easy to make an
application that does exactly that.

## Where's the GUI?
No GUI yet, possibly never. A GUI makes the application a lot larger
and more complex, and on top of that, there isn't much Rust GUI
support right now. If I do add GUI support, I would probably use
[imgui-rs](https://github.com/imgui-rs/imgui-rs).

## What about macOS?
From my limited testing, the Flipper doesn't show up in a macOS
Bluetooth scan. There's one instance of a person using an alternate
Bluetooth tool to connect the Flipper but I don't know if Flipwire
works.

I don't have a macOS device to test on or to provide builds for, so
until I do, consider Flipwire macOS support completely experimental.

## Flipwire hangs during a download.
Run through the troubleshooting steps below just in case something has
temporarily broken. If the issue still happens, then your Bluetooth
adapter is probably incompatible with the Flipper. Report your adapter
model in a new issue and I'll add it to the list above.

# Troubleshooting
Some common problems include Flipwire not finding the Flipper or
returning an error. Make sure the Flipper is already paired to your
computer.

Note that on Linux, you might need to use `bluetoothctl` instead of
your desktop environment's Bluetooth tool. For example, the KDE
Bluetooth tool refuses to pair to the Flipper.

- Turn Bluetooth on the Flipper off and on again.
- Turn Bluetooth on your computer off and on again.
- Unpair the Flipper from your computer and pair it again.
- Unpair all devices from the Flipper (`Settings->Bluetooth->Forget
  All Paired Devices`) and pair it to your computer again.
- Run Flipwire with the `RUST_LOG=debug` environment variable to see
  if anything odd is happening.
- On Linux, restart the Bluetooth service with `sudo systemctl restart
  bluetooth` (or the equivalent if you're not on systemd). 

This can fix issues like `Error finding Flipper Uwuw2:
le-connection-abort-by-local`. You might also need to remove and
re-pair.

- If it's still not working, you've probably discovered a bug. Create
  a new issue with some output with `RUST_LOG=debug` and a description
  of the problem.

Flipwire is only intended for use with Flippers running official
firmware. If you're using another firmware, you're on your own.

# Adapter Incompatibility
Most Bluetooth adapters work perfectly with the Flipper. Some are more
temperamental than others: an operation that works perfectly on one
might require more finagling and more connect/disconnect cycles to
work on another. Others will only work for some operations.

The least compatible adapters I've found are Intel Stone Peak
WiFi/Bluetooth cards. The Flipper's Bluetooth implementation exhibits
strange compatibility issues with these: the Flipper will disconnect
while it's sending data with disconnect reason `0x08` (connection
supervision timeout reached). This means that you can upload files,
run small commands, and download small files without problems, but you
can't download files bigger than about 5 kB before the Flipper
disconnects. I have no idea why this happens (see below).

If your adapter isn't one of the Stone Peak adapters listed below, and
exhibits issues after pair/unpair and disconnect/connect cycles, open
an issue and we'll add it to the incompatible list.

## Broken adapter list
These are the two models in the Intel Stone Peak series. There are
several models of the 7265, but I've only tested the 802.11ac version
of the 7265. However, given that both the 7265 and 3165 have the
Bluetooth issue, I suspect it's common to the whole series.

- Intel Wireless 7265
- Intel Wireless 3165

I tested the 7265 on Linux and the 3165 on Windows and Linux. Whatever
the problem is, it's independent of OS on both the 3165 and 7265. The
Intel 8265 works (see below), so I think this is specific to Stone
Peak.

## Tested working adapters
Given that the Stone Peak adapters have issues independent of
operating system, I assume that all adapter functionality is
OS-agnostic, so I don't keep track of what OS I test card on.

- Intel Wireless 8265
- Intel AX200
- Intel AX201
- Intel AX211
- Qualcomm Atheros QCA6174
- Cypress CYW43455 (on the Pi 4 and Quartz64 Model B)
- Cypress CYW43438 (on the Pi 3)
- Broadcom BCM20702A0 external USB adapter
- Cambridge Silicon external USB adapter

# Contributing
Like Flipwire? Leave me a star!

If you'd like to test Flipwire on macOS, let me know! I'd love to make
it fully cross-platform.

If you have feature requests, bugs to report, or code to add, open an
issue or pull request.

# Building
Make sure you have `protoc`, the [protobuf
compiler](https://github.com/protocolbuffers/protobuf#protobuf-compiler-installation),
installed and in your PATH. On Linux, you also need `libdbus`
(including the headers) and `pkg-config`. Check your package manager
for these. 

Clone the Flipwire repo and submodules, and run `cargo build`:

```
$ git clone --recursive https://github.com/liamhays/flipwire
$ cd flipwire
$ cargo build
```

If you're on Linux, especially a weak single-board computer, I
recommend using the [mold](https://github.com/rui314/mold) linker
via `mold -run` or some configuration in `.cargo/config.toml`.
