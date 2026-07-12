{
  description = "comfyc dev shell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
        armCross = pkgs.pkgsCross.armv7l-hf-multiplatform;
        armBinutils = armCross.buildPackages.binutils;
        crossPrefix = armCross.stdenv.hostPlatform.config + "-";
      in
      {
        devShells.default = pkgs.mkShell {
          packages = [
            pkgs.rustc
            pkgs.cargo
            pkgs.rustfmt
            pkgs.clippy
            pkgs.rust-analyzer
            armBinutils
            pkgs.qemu
          ];

          COMFYC_CROSS_PREFIX = crossPrefix;
          QEMU_ARM = "${pkgs.qemu}/bin/qemu-arm";

          shellHook = ''
            echo "comfyc dev shell"
            echo "cross prefix: ${crossPrefix}"
            echo "qemu-arm: $QEMU_ARM"
          '';
        };
      }
    );
}
