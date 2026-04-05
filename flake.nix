{
  inputs = {
    naersk.url = "github:nix-community/naersk/master";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      utils,
      naersk,
    }:
    utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
        naersk-lib = pkgs.callPackage naersk { };
      in
      {
        defaultPackage = naersk-lib.buildPackage ./.;
        devShell =
          with pkgs;
          mkShell {
            nativeBuildInputs = [ rustup ];
            buildInputs = [
              cargo
              ninja
              rust-script
              cmake
              zstd
              gawk
              gnugrep
              gnutar
              wget
              which
              findutils
              gzip
              xz
              util-linux
              procps
              perl
              file
              pkg-config
              libarchive
              openssl
              lld
              zlib
              rustc
              rustfmt
              gmp
              mpfr
              libmpc
              stdenv.cc.cc.lib
              flex
              pkgsCross.x86_64-embedded.buildPackages.gcc
              bison
              autoconf
              automake
              texinfo
              pre-commit
              meson
              rustPackages.clippy
              qemu
              codex
            ];
            RUST_SRC_PATH = rustPlatform.rustLibSrc;
            shellHook = ''
              export REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
              export SYSROOT_DIR="$REPO_ROOT/sysroot"
              export TOOLCHAIN_DIR="$REPO_ROOT/toolchain"
              export LLVM_BUILD_BIN="$TOOLCHAIN_DIR/llvm-project/build-seele/bin"
              export LD_LIBRARY_PATH=${pkgs.zstd.out}/lib:${pkgs.zlib}/lib:${pkgs.stdenv.cc.cc.lib}/lib:$LD_LIBRARY_PATH
              export PATH="$REPO_ROOT/.llvm/bin:$HOME/.cargo/bin:$PATH"
              echo "[devshell] Ensuring Rust toolchain 'seele'..."
              #(cd "$TOOLCHAIN_DIR" && ./install.rs) || echo "[devshell] install.rs failed"
              sudo mount disk.img sysroot
            '';
          };
      }
    );
}
