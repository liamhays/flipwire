**Hi! If you came across this repo by chance and are reading this sentence, be aware some of the information below might be inaccurate.**

# flipwire - Cross-Platform Bluetooth Flipper Control
Flipwire lets you control your Flipper Zero from any computer over
Bluetooth just like the mobile app. Flipwire is currently only a
command-line tool.

I am currently only providing Linux and Windows builds of Flipwire,
because I don't have a macOS machine to build and test on. If you
would like to be a macOS tester, open an issue.

# Usage
Flipwire will attempt to connect to your Flipper before any
operation. Pair your Flipper to your computer **before** using
Flipwire.

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

TODO: path issue things

# Adapter Incompatibility
In RPC mode, the Flipper's Bluetooth implementation exhibits strange
compatibility issues with Intel Bluetooth adapters: the Flipper will
disconnect while it's sending data with disconnect reason `0x08`
(connection supervision timeout reached).

I *believe* that this is the fault of the core2 coprocessor on the
STM32WB55, since the Intel adapters work with every other device.

## Known broken adapters
- Intel Wireless 7265 (tested on Linux)
- Intel Wireless 3165 (tested on Windows)

## Known working adapters
- Qualcomm Atheros QCA6174
- Cambridge Silicon external USB adapter
- AzureWave AW-CM256SM (using CYW43455, found on the Quartz64 Model B)
- Broadcom BCM4345/6 (found on Raspberry Pi 4)

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
- Run Flipwire with the `RUST_LOG` environment variable set to `debug`
  to see if anything odd is happening.
- By now, you have spent more time fixing Bluetooth than it would have
  taken you to grab a USB-C cable and use qFlipper.
  
If you think you've discovered a bug, post some output and a
description of the problem in a new issue.

# Contributing
As mentioned above, I need a macOS tester to really have
cross-platform support.

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
