# Proteus dev shell (PRD §4): reproducible NixOS environment.
# Rust toolchain + the test-discipline tools (PRD §5/§7) + the graphics/system
# libraries gpui/gpui-component need to build and run on NixOS (declared here
# explicitly, since NixOS doesn't provide them system-wide like other distros).
{
  description = "Proteus — local-first PDF & image desktop app (dev shell)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
    in
    {
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
          fontconfig
          freetype
          libGL
          libxkbcommon
          openssl
          vulkan-loader
          wayland
          libxcb
          zlib
        ];

        shellHook = ''
          # cargo-llvm-cov needs llvm-cov/llvm-profdata matching rustc's bundled LLVM
          export LLVM_COV="${pkgs.llvm_21}/bin/llvm-cov"
          export LLVM_PROFDATA="${pkgs.llvm_21}/bin/llvm-profdata"
          # gpui runtime: Vulkan + GL must be on LD_LIBRARY_PATH for binaries built outside nix.
          export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath [
            pkgs.vulkan-loader
            pkgs.libGL
            pkgs.fontconfig
            pkgs.freetype
            pkgs.libxkbcommon
            pkgs.wayland
            pkgs.libxcb
            pkgs.zlib
          ]}"
          echo "Proteus dev shell — cargo test / cargo llvm-cov / cargo mutants ready (proteus-core)."
        '';
      };
    };
}
