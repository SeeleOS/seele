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
              clang
              lld
              rustc
              rustfmt
              gmp
              mpfr
              libmpc
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
            ];
            RUST_SRC_PATH = rustPlatform.rustLibSrc;
            shellHook = ''
                                 export SYSROOT_DIR="/home/elysia/coding-project/seeleos/sysroot"
              		      export TOOLCHAIN_DIR="/home/elysia/coding-project/seeleos/toolchain"
                                    export RUSTUP_DIST_SERVER=https://mirrors.tuna.tsinghua.edu.cn/rustup
                                    export PATH=~/.cargo/bin:$TOOLCHAIN_DIR/misc/toolchain/bin:$PATH
                                                                     	  '';
          };
      }
    );
}
