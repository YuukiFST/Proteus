# Proteus dev shell (PRD §4): reproducible NixOS environment.
# Rust toolchain + the test-discipline tools (PRD §5/§7) + the graphics/system
# libraries gpui/gpui-component need to build and run on NixOS (declared here
# explicitly, since NixOS doesn't provide them system-wide like other distros).
#
# Also provides the `proteus` package: wraps the prebuilt Linux binary from the
# GitHub release so NixOS users can run it with `nix run github:YuukiFST/Proteus`.
# The GitHub binary is built against Ubuntu glibc and links libxcb/libxkbcommon
# dynamically, so autoPatchelfHook fixes the ELF interpreter and RPATH, and
# runtimeDependencies keeps dlopen()-ed libs (vulkan-loader, libGL) findable.
#
# To update after publishing a new release:
#   new_hash=$(nix hash file --sri <(curl -Ls <asset-url>))
#   bump version + linuxHash below.
{
  description = "Proteus — local-first PDF & image desktop app (dev shell + NixOS package)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };

      # gpui runtime libs, declared once, reused by devShell and the package.
      gpuiLibs = with pkgs; [
        fontconfig
        freetype
        libGL
        libxkbcommon
        wayland
        libxcb
        zlib
        vulkan-loader
      ];

      proteusVersion = "0.1.4";
      # SRI hash of the release asset proteus-linux (see header comment).
      proteusLinuxHash = "sha256-9lWgzWwwxU8auTnFn72BPsNSReo4jpQrIj4kNgVJtEo=";
    in
    {
      packages.${system} = {
        proteus = pkgs.stdenv.mkDerivation {
          pname = "proteus";
          version = proteusVersion;
          src = pkgs.fetchurl {
            url = "https://github.com/YuukiFST/Proteus/releases/download/v${proteusVersion}/proteus-linux";
            hash = proteusLinuxHash;
          };
          dontUnpack = true;
          nativeBuildInputs = [ pkgs.autoPatchelfHook ];
          # libgcc_s needed by the Ubuntu-built binary's Rust runtime.
          buildInputs = gpuiLibs ++ [ pkgs.stdenv.cc.cc.lib ];
          # dlopen()-ed at runtime (Vulkan loader, GL), not DT_NEEDED: keep them
          # on RPATH via runtimeDependencies, like nixpkgs does for wrapper-less
          # prebuilt binaries.
          runtimeDependencies = gpuiLibs;
          installPhase = ''
            runHook preInstall
            install -Dm755 $src $out/bin/proteus
            runHook postInstall
          '';
          meta = with pkgs.lib; {
            description = "Local-first PDF & image desktop app";
            homepage = "https://github.com/YuukiFST/Proteus";
            license = licenses.mit;
            platforms = [ "x86_64-linux" ];
            mainProgram = "proteus";
          };
        };
        default = self.packages.${system}.proteus;
      };

      apps.${system}.default = {
        type = "app";
        program = "${self.packages.${system}.proteus}/bin/proteus";
      };

      devShells.${system}.default = pkgs.mkShell {
        packages = with pkgs; [
          # Rust toolchain (nixpkgs-pinned: rustc + cargo must match each other)
          cargo
          rustc
          clippy
          rustfmt

          # Test arsenal (PRD §5/§7): coverage + mutation gates.
          # cargo-llvm-cov needs llvm-cov/llvm-profdata matching rustc's bundled LLVM
          # (nixpkgs' wrapper doesn't set them; rustc 1.95 bundles LLVM 21.1.8).
          cargo-llvm-cov
          cargo-mutants
          llvm_21

          # Build plumbing
          pkg-config

          # gpui / gpui-component system libraries (Linux build + run)
        ] ++ gpuiLibs ++ [ pkgs.openssl ];

        shellHook = ''
          # cargo-llvm-cov needs llvm-cov/llvm-profdata matching rustc's bundled LLVM
          export LLVM_COV="${pkgs.llvm_21}/bin/llvm-cov"
          export LLVM_PROFDATA="${pkgs.llvm_21}/bin/llvm-profdata"
          # gpui runtime: Vulkan + GL must be on LD_LIBRARY_PATH for binaries built outside nix.
          export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath (gpuiLibs ++ [ pkgs.openssl ])}"
          echo "Proteus dev shell — cargo test / cargo llvm-cov / cargo mutants ready (proteus-core)."
        '';
      };
    };
}