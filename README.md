# flipwire - Cross-Platform Bluetooth Flipper Control

![demo GIF of Flipwire](docs/demo.gif)

Flipwire lets you control your Flipper Zero from any computer over
Bluetooth just like the mobile app. Flipwire is currently only a
command-line tool.

I am currently only providing Linux and Windows builds of Flipwire,
because I don't have a macOS machine to build and test on. If you
would like to be a macOS tester, let me know by opening an issue.

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

# FAQ
## Why "flipwire"?
It sounds cool, plus "flip" is kind of a synonym of "invert", and the
opposite of a wire is wireless, just like `flipwire` and the Flipper.

## Why Rust?
Because I like Rust. Also, because the Flipper ecosystem has a strong
"plug and play" mentality, and Rust makes it easy to make an
application that does exactly that.

## Where's the GUI?
No GUI yet, possibly never. A GUI makes the application a lot larger
and more complex, and on top of that, there isn't much Rust GUI
support right now. If I do add GUI support, I would probably use
[imgui-rs](https://github.com/imgui-rs/imgui-rs).

## Flipwire hangs during a download.
Run through the troubleshooting steps below just in case something has
temporarily broken. If the issue still happens, then your Bluetooth
adapter is probably incompatible with the Flipper. Report your adapter
model in a new issue and I'll add it to the list above.

# Troubleshooting
Some common problems include Flipwire not finding the Flipper or
returning an error.

- Make sure the Flipper is already paired to your computer. 

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

Please note that I will only support Flipwire when used with Flippers
running official firmware.

# Adapter Incompatibility
In RPC mode, the Flipper's Bluetooth implementation exhibits strange
compatibility issues with Stone Peak series Intel Bluetooth adapters:
the Flipper will disconnect while it's sending data with disconnect
reason `0x08` (connection supervision timeout reached). This means
that you can upload files, run small commands, and download small
files without problems, but you can't download files bigger than about
5 kB before the Flipper disconnects.

If your adapter doesn't work, Flipwire will hang when it tries to
download a file, because it's waiting for more data from a
disconnected device. I *believe* that this is the fault of the core2
coprocessor on the STM32WB55, since the Intel adapters work with every
other device.

## Broken adapters
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

I need a macOS tester to have real cross-platform support. I've done a
little bit of macOS testing, and the system Bluetooth scanner wouldn't
find my Flipper.

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
