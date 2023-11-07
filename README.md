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


# Adapter Incompatibility
In RPC mode, the Flipper's Bluetooth implementation exhibits strange
compatibility issues with Intel Bluetooth adapters: the Flipper will
disconnect while it's sending data with disconnect reason `0x08`
(connection supervision timeout reached). This means that you can
upload files and run small commands fine, but you can't download files
from the Flipper.

If your adapter doesn't work, Flipwire will hang when it tries to
download a file, because it's waiting for more data from a
disconnected device.

I *believe* that this is the fault of the core2 coprocessor on the
STM32WB55, since the Intel adapters work with every other device.

## Known broken adapters
- Intel Wireless 7265 (tested on Linux)
- Intel Wireless 3165 (tested on Windows)

## Known working adapters
- Qualcomm Atheros QCA6174
- Cambridge Silicon external USB adapter
- Cypress CYW43455 (Pi 4 and Quartz64 Model B)
- Cypress CYW43438 (Pi 3)

# FAQ
## Why "flipwire"?
It sounds cool, plus "flip" is kind of a synonym of "invert", and the
opposite of a wire is wireless, just like `flipwire` and the Flipper.

## Why Rust?
Because I like Rust. Also, because the Flipper ecosystem has a strong
"plug and play" mentality, and Rust makes it easy to make an
application that does exactly that.

# Troubleshooting
- Make sure the Flipper is already paired to your computer.
- Turn Bluetooth on the Flipper off and on again.
- Turn Bluetooth on your computer off and on again.
- Unpair the Flipper from your computer and pair it again.
- Unpair all devices from the Flipper (`Settings->Bluetooth->Forget
  All Paired Devices`) and pair it to your computer again.
- Run Flipwire with the `RUST_LOG=debug` environment variable to see
  if anything odd is happening.
- By now, you have spent more time fixing Bluetooth than it would have
  taken you to grab a USB-C cable and use qFlipper. Use qFlipper or
  your phone and copy files from there.
  
If you think you've discovered a bug, create a new issue with some
output with `RUST_LOG=debug` and a description of the problem.

# Contributing
Like Flipwire? Leave me a star!

I need a macOS tester to have real cross-platform support.

If you have feature requests, bugs to report, or code to add, open an
issue or pull request.

# Building
Make sure you have `protoc`, the protobuf compiler, installed and in
your PATH. Clone the Flipwire repo and submodules, and run `cargo build`:

```
$ git clone --recursive https://github.com/liamhays/flipwire
$ cd flipwire
$ cargo build
```

If you're on Linux, especially a weak single-board computer, I
recommend using the (https://github.com/rui314/mold)[mold] linker
via `mold -run` or some configuration in `.cargo/config.toml`.
