{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    {
      self,
      flake-utils,
      nixpkgs,
      rust-overlay,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        runPackages = with pkgs; [
          bash
          coreutils
          e2fsprogs
          git
          pacman
          qemu
          util-linux
          toolchain
        ];

        devPackages = with pkgs; [
          e2fsprogs
          git
          pacman
          procps
          qemu
          util-linux
          toolchain
        ];

        runApp = pkgs.writeShellApplication {
          name = "seele-run";
          runtimeInputs = runPackages;
          text = ''
            set -eu

            repo_root="$(${pkgs.git}/bin/git rev-parse --show-toplevel 2>/dev/null || ${pkgs.coreutils}/bin/pwd -P)"
            cd "$repo_root"

            needs_rootfs_init=0
            if [ ! -f disk.img ]; then
              needs_rootfs_init=1
            elif ! ${pkgs.util-linux}/bin/mountpoint -q sysroot 2>/dev/null && [ ! -e sysroot/bin/bash ]; then
              needs_rootfs_init=1
            fi

            if [ "$needs_rootfs_init" -eq 1 ]; then
              ./misc/make_disk.sh
            fi

            exec cargo run -- "$@"
          '';
        };

        defaultDevShell = pkgs.mkShell {
          packages = devPackages;
        };
      in
      {
        packages.default = runApp;
        apps.default = {
          type = "app";
          program = "${runApp}/bin/seele-run";
        };
        devShells.default = defaultDevShell;

        defaultPackage = runApp;
        devShell = defaultDevShell;
      }
    );
}
