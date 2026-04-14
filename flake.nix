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
              cargo-c
              autoconf
              automake
              bison
              cmake
              file
              findutils
              flex
              gawk
              desktop-file-utils
              gettext
              glib
              gdk-pixbuf
              gtk4
              libgee
              libadwaita
              librsvg
              libgnome-games-support
              shared-mime-info
              gnugrep
              gnutar
              gperf
              gzip
              libarchive
              libtool
              lld
              libmpc
              meson
              mpfr
              ninja
              openssl
              perl
              pkg-config
              pkgsCross.x86_64-embedded.buildPackages.gcc
              pre-commit
              procps
              python3
              qemu
              rust-script
              rustPackages.clippy
              rustc
              rustfmt
              sassc
              stdenv.cc.cc.lib
              appstream
              texinfo
              itstool
              util-linux
              vala
              wget
              which
              xz
              yelp-tools
              zlib
              zstd
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
