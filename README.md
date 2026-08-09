# Proteus

Local-first desktop app for PDF and image workflows. UI in gpui; parsing and
protection logic in `proteus-core`.

## Installing

Prebuilt binaries are attached to each GitHub release:
https://github.com/YuukiFST/Proteus/releases

### Windows

Download `proteus-windows.exe` — run directly. Requires Windows 10/11 and a
GPU driver with DX12 or Vulkan (most machines qualify).

### Linux (Debian/Ubuntu/Fedora/Arch…)

Download `proteus-linux` and run it:

```sh
chmod +x proteus-linux
./proteus-linux
```

The binary links common X11/graphics libraries dynamically. On a minimal
system, install them first:

```sh
# Debian/Ubuntu
sudo apt install libxkbcommon-x11-0 libxcb1 libgl1 fontconfig
# Fedora
sudo dnf install libxkbcommon-x11 libxcb libGL fontconfig
```

### NixOS

The bundled binary is built on Ubuntu, so NixOS presents it via the flake's
`proteus` package (patchelf + runtime libs handled for you):

```sh
nix run github:YuukiFST/Proteus
```

Requires Nix flakes and a Vulkan driver in your system (desktop default).

## Building from source

```sh
cargo build --release -p proteus-app
# binary at target/release/proteus
```

On NixOS use the dev shell (declares the gpui system libraries):

```sh
nix develop
```

## Tests

```sh
cargo test -p proteus-core        # unit + adversarial PDF oracles
cargo llvm-cov                    # coverage floor (80% on proteus-core)
cargo mutants                     # mutation gate (T2 pdf_protect surface)
```