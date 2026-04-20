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
          git
          qemu
          util-linux
          toolchain
        ];

        devPackages = with pkgs; [
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
