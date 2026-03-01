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
              pkgs.pkgsCross.x86_64-embedded.buildPackages.gcc
              clang
              lld
              rustc
              rustfmt
              pre-commit
              meson
              rustPackages.clippy
              qemu
            ];
            RUST_SRC_PATH = rustPlatform.rustLibSrc;
            shellHook = ''
                            	  export RUSTUP_DIST_SERVER=https://mirrors.tuna.tsinghua.edu.cn/rustup
              		  export PATH=~/.cargo/bin:$PATH
                            	  '';
          };
      }
    );
}
